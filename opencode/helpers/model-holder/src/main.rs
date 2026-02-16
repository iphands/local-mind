use clap::{Parser, Subcommand};
use glob::glob;
use log::{debug, error, info, warn};
use memmap2::Mmap;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Constants for magic numbers and configuration
const DEFAULT_PAGE_SIZE: usize = 4096;
const PROGRESS_UPDATE_INTERVAL: usize = 1000;
const PROGRESS_UPDATE_TIMEOUT_MS: u64 = 100;
const SIZE_MISMATCH_THRESHOLD: f64 = 0.1; // 10% threshold for size mismatch
const MAX_CMDLINE_DISPLAY_LENGTH: usize = 80;
const BYTES_PER_MB: f64 = 1_048_576.0;
const BYTES_PER_GB: f64 = 1_073_741_824.0;

/// Default model file extensions
const DEFAULT_EXTENSIONS: &str = "gguf,ggml,bin";

/// Process names to search for
const MODEL_HOLDER_PROCESS_NAME: &str = "model-holder";
const LLAMA_PROCESS_NAMES: &[&str] = &["llama-server", "llama-cli", "llama.cpp"];

/// Errors specific to model-holder operations
#[derive(Error, Debug)]
enum ModelHolderError {
    #[error("Invalid page size: {0}. Must be a positive power of 2.")]
    InvalidPageSize(usize),

    #[error("Invalid extension format: {0}. Must be comma-separated list without dots.")]
    InvalidExtensions(String),

    #[error("No files matched pattern(s): {0}")]
    NoFilesFound(String),

    #[error(
        "Permission denied accessing /proc data. Run as root or ensure processes run as same user."
    )]
    PermissionDenied(Vec<u32>),

    #[error("Path traversal detected: {0}")]
    PathTraversal(String),

    #[error("Process {0} disappeared during analysis")]
    ProcessDisappeared(u32),
}

impl From<ModelHolderError> for io::Error {
    fn from(err: ModelHolderError) -> Self {
        io::Error::new(io::ErrorKind::InvalidInput, err)
    }
}

impl From<io::Error> for ModelHolderError {
    fn from(err: io::Error) -> Self {
        ModelHolderError::NoFilesFound(format!("IO error: {}", err))
    }
}

/// Common arguments for hold operations
#[derive(Parser, Debug, Clone)]
struct HoldArgs {
    /// Path(s) to model files - supports glob patterns (e.g., "model-*.gguf")
    #[arg(required = true)]
    model_paths: Vec<String>,

    /// Skip warming up pages (don't touch all pages after mmap)
    #[arg(long, default_value_t = false)]
    no_warmup: bool,

    /// Page size for warmup reads (must be positive power of 2)
    #[arg(long, default_value_t = DEFAULT_PAGE_SIZE)]
    page_size: usize,
}

#[derive(Parser, Debug)]
#[command(name = "model-holder")]
#[command(about = "Keeps model files mmap'd in memory to speed up llama.cpp restarts")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path(s) to model files - supports glob patterns (e.g., "model-*.gguf")
    /// Used when no subcommand is specified (legacy mode)
    #[arg(required = false)]
    model_paths: Vec<String>,

    /// Skip warming up pages (don't touch all pages after mmap)
    #[arg(long, default_value_t = false)]
    no_warmup: bool,

    /// Page size for warmup reads (must be positive power of 2)
    #[arg(long, default_value_t = DEFAULT_PAGE_SIZE)]
    page_size: usize,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Hold model files in memory (same as running without subcommand)
    Hold(HoldArgs),

    /// Check mmap status of model-holder and llama-server processes
    Check {
        /// Model file extensions to look for (comma-separated, no dots)
        #[arg(long, default_value = DEFAULT_EXTENSIONS)]
        extensions: String,
    },
}

/// Validate page size is a positive power of 2
fn validate_page_size(page_size: usize) -> Result<(), ModelHolderError> {
    if page_size == 0 || !page_size.is_power_of_two() {
        return Err(ModelHolderError::InvalidPageSize(page_size));
    }
    Ok(())
}

/// Validate extensions format (comma-separated, no dots)
fn validate_extensions(extensions: &str) -> Result<(), ModelHolderError> {
    if extensions.is_empty() {
        return Err(ModelHolderError::InvalidExtensions(extensions.to_string()));
    }

    for ext in extensions.split(',') {
        let ext = ext.trim();
        if ext.is_empty() || ext.starts_with('.') || ext.contains('.') {
            return Err(ModelHolderError::InvalidExtensions(extensions.to_string()));
        }
    }
    Ok(())
}

/// Sanitize file path to prevent path traversal attacks
/// Note: This does NOT canonicalize - that happens after glob expansion
fn sanitize_path(path: &str) -> Result<PathBuf, ModelHolderError> {
    let path = Path::new(path);

    // Check for path traversal attempts
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(ModelHolderError::PathTraversal(
            path.to_string_lossy().into_owned(),
        ));
    }

    // Make absolute if relative, but don't canonicalize yet
    // (canonicalization happens after glob expansion to properly resolve symlinks)
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|e| {
                ModelHolderError::NoFilesFound(format!("Cannot get current directory: {}", e))
            })?
            .join(path))
    }
}

/// A memory-mapped file with its metadata
#[derive(Debug)]
struct MappedFile {
    path: PathBuf,
    mmap: Mmap,
    size: u64,
}

/// File identity based on device and inode (for cross-mount comparison)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileIdentity {
    dev: String,
    inode: u64,
}

/// A memory-mapped region from /proc/pid/maps
#[derive(Debug, Clone)]
struct MappedRegion {
    path: String,
    start: u64,
    end: u64,
    dev: String,
    inode: u64,
}

/// Process information with memory mappings
#[derive(Debug)]
struct ProcessMaps {
    pid: u32,
    comm: String,
    cmdline: String,
    maps: Vec<MappedRegion>,
}

/// Aggregated file information combining multiple mapped regions
#[derive(Debug, Clone)]
struct AggregatedFile {
    path: String,
    identity: FileIdentity,
    mapped_size: u64,
}

/// Expand glob patterns and validate/sanitize paths
fn expand_globs(patterns: &[String]) -> Result<Vec<PathBuf>, ModelHolderError> {
    let mut paths = Vec::new();

    for pattern in patterns {
        let sanitized_pattern = sanitize_path(pattern)?;
        let pattern_str = sanitized_pattern.to_string_lossy();

        debug!("Expanding pattern: {}", pattern_str);

        let matches: Vec<_> = glob(&pattern_str)
            .map_err(|e| {
                ModelHolderError::NoFilesFound(format!("Invalid glob '{}': {}", pattern_str, e))
            })?
            .filter_map(Result::ok)
            .collect();

        if matches.is_empty() {
            // If glob didn't match, try treating it as a direct file path
            // But only if the file exists
            if sanitized_pattern.exists() {
                warn!(
                    "No glob matches for '{}', treating as direct path",
                    pattern_str
                );
                paths.push(sanitized_pattern);
            } else {
                return Err(ModelHolderError::NoFilesFound(format!(
                    "File not found: '{}' (resolved to '{}')",
                    pattern,
                    pattern_str
                )));
            }
        } else {
            info!(
                "Found {} files matching pattern: {}",
                matches.len(),
                pattern_str
            );
            paths.extend(matches);
        }
    }

    // Canonicalize all paths to resolve symlinks and relative paths
    debug!("Canonicalizing {} path(s)", paths.len());
    let canonical_paths: Result<Vec<PathBuf>, _> = paths
        .into_iter()
        .map(|p| {
            fs::canonicalize(&p).map_err(|e| {
                ModelHolderError::NoFilesFound(format!(
                    "Cannot resolve path '{}': {}",
                    p.display(),
                    e
                ))
            })
        })
        .collect();

    let mut canonical_paths = canonical_paths?;
    canonical_paths.sort();
    canonical_paths.dedup();

    if canonical_paths.is_empty() {
        Err(ModelHolderError::NoFilesFound(patterns.join(", ")))
    } else {
        Ok(canonical_paths)
    }
}

/// Warm up memory-mapped file by reading all pages
///
/// # Safety
/// This function safely accesses memory-mapped data by:
/// 1. Using the mmap's length to bound all accesses
/// 2. Only reading within the mapped region
/// 3. Using step_by to ensure proper page alignment
fn warmup_file(
    mmap: &Mmap,
    file_size: u64,
    page_size: usize,
    file_idx: usize,
    total_files: usize,
) -> io::Result<u64> {
    let total_pages = (file_size as usize + page_size - 1) / page_size;
    let mut checksum: u64 = 0;
    let start_time = Instant::now();
    let mut last_print_time = Instant::now();

    for (page_index, offset) in (0..mmap.len()).step_by(page_size).enumerate() {
        // Safety: mmap.len() ensures we don't read beyond mapped region
        checksum = checksum.wrapping_add(mmap[offset] as u64);

        if page_index % PROGRESS_UPDATE_INTERVAL == 0
            || last_print_time.elapsed() > Duration::from_millis(PROGRESS_UPDATE_TIMEOUT_MS)
        {
            let elapsed = start_time.elapsed().as_secs_f64();
            let bytes_done = (page_index * page_size) as f64;
            let mb_per_sec = if elapsed > 0.0 {
                bytes_done / elapsed / BYTES_PER_MB
            } else {
                0.0
            };
            let percent = (page_index as f64 / total_pages as f64) * 100.0;

            print!(
                "\r[{}/{}] Progress: {:5.1}% | {:7.1} MB/s",
                file_idx + 1,
                total_files,
                percent,
                mb_per_sec
            );
            io::stdout().flush()?;
            last_print_time = Instant::now();
        }
    }

    let elapsed = start_time.elapsed().as_secs_f64();
    let mb_per_sec = if elapsed > 0.0 {
        file_size as f64 / elapsed / BYTES_PER_MB
    } else {
        0.0
    };

    println!(
        "\r[{}/{}] Complete: {:.2}s @ {:.1} MB/s (checksum: {:#x})        ",
        file_idx + 1,
        total_files,
        elapsed,
        mb_per_sec,
        checksum
    );

    Ok(checksum)
}

/// Hold model files in memory with optional warmup
fn hold_models(
    model_paths: Vec<String>,
    no_warmup: bool,
    page_size: usize,
) -> Result<(), ModelHolderError> {
    // Validate inputs
    validate_page_size(page_size)?;

    let paths = expand_globs(&model_paths)?;
    info!("Found {} files to memory map", paths.len());

    println!("Files to mmap ({}):", paths.len());
    for path in &paths {
        println!("  {}", path.display());
    }
    println!();

    let mut mapped_files: Vec<MappedFile> = Vec::new();
    let mut total_size: u64 = 0;

    for path in &paths {
        let file = File::open(path).map_err(|e| {
            ModelHolderError::NoFilesFound(format!("Cannot open '{}': {}", path.display(), e))
        })?;

        let metadata = file.metadata().map_err(|e| {
            ModelHolderError::NoFilesFound(format!(
                "Cannot read metadata for '{}': {}",
                path.display(),
                e
            ))
        })?;

        let size = metadata.len();
        total_size += size;

        // Safety: Mmap::map is safe here because we're mapping a regular file that we just opened
        // The mmap will be automatically dropped when mapped_files goes out of scope
        let mmap = unsafe { Mmap::map(&file) }.map_err(|e| {
            ModelHolderError::NoFilesFound(format!("Cannot mmap '{}': {}", path.display(), e))
        })?;

        println!(
            "mmap'd: {} ({:.2} GB) at {:p}",
            path.display(),
            size as f64 / BYTES_PER_GB,
            mmap.as_ptr()
        );

        mapped_files.push(MappedFile {
            path: path.clone(),
            mmap,
            size,
        });
    }

    println!(
        "\nTotal: {:.2} GB across {} file(s)",
        total_size as f64 / BYTES_PER_GB,
        mapped_files.len()
    );

    if !no_warmup {
        println!("\nWarming up pages...");
        let warmup_start_time = Instant::now();

        for (file_index, mapped_file) in mapped_files.iter().enumerate() {
            let filename = mapped_file
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown");

            print!("  {} ", filename);
            io::stdout().flush()?;

            warmup_file(
                &mapped_file.mmap,
                mapped_file.size,
                page_size,
                file_index,
                mapped_files.len(),
            )
            .map_err(|e| {
                ModelHolderError::NoFilesFound(format!(
                    "Warmup failed for '{}': {}",
                    mapped_file.path.display(),
                    e
                ))
            })?;
        }

        let total_elapsed = warmup_start_time.elapsed().as_secs_f64();
        let total_mb_per_sec = if total_elapsed > 0.0 {
            total_size as f64 / total_elapsed / BYTES_PER_MB
        } else {
            0.0
        };

        println!(
            "\nAll files warmed up in {:.2}s (avg {:.1} MB/s)",
            total_elapsed, total_mb_per_sec
        );
    } else {
        println!("\nSkipping warmup (pages will be loaded on demand)");
    }

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    ctrlc::set_handler(move || {
        println!("\nReceived interrupt, releasing mmaps and exiting...");
        running_clone.store(false, Ordering::SeqCst);
    })
    .map_err(|e| ModelHolderError::NoFilesFound(format!("Cannot set Ctrl+C handler: {}", e)))?;

    println!(
        "\nHolding {} file(s) in memory. Press Ctrl+C to release and exit.",
        mapped_files.len()
    );
    println!("Other processes using --mmap on these files will share these pages.");

    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }

    // RAII: mapped_files are automatically dropped here, releasing the memory mappings
    drop(mapped_files);
    println!("Done.");

    Ok(())
}

// === Check command implementation ===

/// Find processes by name, searching /proc for matching process names
fn find_processes_by_name(names: &[&str]) -> Result<Vec<u32>, ModelHolderError> {
    let mut found_pids = Vec::new();
    let my_pid = std::process::id();

    debug!("Searching for processes with names: {:?}", names);

    for entry in fs::read_dir("/proc").map_err(|e| {
        ModelHolderError::NoFilesFound(format!("Cannot read /proc directory: {}", e))
    })? {
        let entry = entry.map_err(|e| {
            ModelHolderError::NoFilesFound(format!("Error reading /proc entry: {}", e))
        })?;
        let filename = entry.file_name();
        let filename_str = filename.to_string_lossy();

        if let Ok(pid) = filename_str.parse::<u32>() {
            if pid == my_pid {
                continue;
            }

            let comm_path = format!("/proc/{}/comm", pid);
            if let Ok(comm) = fs::read_to_string(&comm_path) {
                let comm = comm.trim();
                for target_name in names {
                    if comm.contains(target_name) {
                        debug!("Found matching process: PID {} ({})", pid, comm);
                        found_pids.push(pid);
                        break;
                    }
                }
            }
        }
    }

    Ok(found_pids)
}

/// Read memory maps for a process, handling race conditions
fn read_process_maps(pid: u32) -> Result<ProcessMaps, ModelHolderError> {
    let comm_path = format!("/proc/{}/comm", pid);
    let cmdline_path = format!("/proc/{}/cmdline", pid);
    let maps_path = format!("/proc/{}/maps", pid);

    let comm = fs::read_to_string(&comm_path)
        .unwrap_or_else(|_| {
            warn!("Process {} disappeared while reading comm", pid);
            String::new()
        })
        .trim()
        .to_string();

    let cmdline = fs::read_to_string(&cmdline_path)
        .unwrap_or_else(|_| {
            warn!("Process {} disappeared while reading cmdline", pid);
            String::new()
        })
        .replace('\0', " ")
        .trim()
        .to_string();

    let file = File::open(&maps_path).map_err(|e| {
        if e.kind() == io::ErrorKind::PermissionDenied {
            ModelHolderError::PermissionDenied(vec![pid])
        } else if e.kind() == io::ErrorKind::NotFound {
            ModelHolderError::ProcessDisappeared(pid)
        } else {
            ModelHolderError::NoFilesFound(format!("Cannot read maps for PID {}: {}", pid, e))
        }
    })?;

    let reader = io::BufReader::new(file);
    let mut maps = Vec::new();

    for line_result in reader.lines() {
        let line = line_result.map_err(|e| {
            ModelHolderError::NoFilesFound(format!("Error reading maps for PID {}: {}", pid, e))
        })?;

        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.len() >= 6 {
            let addr_parts: Vec<&str> = parts[0].split('-').collect();
            if addr_parts.len() == 2 {
                if let (Ok(start), Ok(end)) = (
                    u64::from_str_radix(addr_parts[0], 16),
                    u64::from_str_radix(addr_parts[1], 16),
                ) {
                    let dev = parts[3].to_string();
                    let inode = parts[4].parse::<u64>().unwrap_or(0);
                    let path = parts[5..].join(" ");

                    // Only include actual file paths (not [heap], [stack], etc.)
                    if path.starts_with('/') && inode > 0 {
                        maps.push(MappedRegion {
                            path,
                            start,
                            end,
                            dev,
                            inode,
                        });
                    }
                }
            }
        }
    }

    Ok(ProcessMaps {
        pid,
        comm,
        cmdline,
        maps,
    })
}

/// Check if a file path matches model file extensions (case-insensitive)
fn is_model_file(path: &str, extensions: &HashSet<&str>) -> bool {
    if let Some(dot_pos) = path.rfind('.') {
        let extension = &path[dot_pos + 1..];
        extensions.contains(&extension.to_lowercase().as_str())
    } else {
        false
    }
}

fn aggregate_maps(maps: &[MappedRegion]) -> Vec<AggregatedFile> {
    let mut by_identity: HashMap<FileIdentity, (String, u64)> = HashMap::new();

    for region in maps {
        let identity = FileIdentity {
            dev: region.dev.clone(),
            inode: region.inode,
        };
        let size = region.end - region.start;
        by_identity
            .entry(identity)
            .and_modify(|(_, total)| *total += size)
            .or_insert((region.path.clone(), size));
    }

    by_identity
        .into_iter()
        .map(|(identity, (path, mapped_size))| AggregatedFile {
            path,
            identity,
            mapped_size,
        })
        .collect()
}

/// Format byte count as human-readable string
fn format_size(bytes: u64) -> String {
    if bytes >= BYTES_PER_GB as u64 {
        format!("{:.2} GB", bytes as f64 / BYTES_PER_GB)
    } else if bytes >= BYTES_PER_MB as u64 {
        format!("{:.2} MB", bytes as f64 / BYTES_PER_MB)
    } else if bytes >= 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Check process permissions before attempting analysis
fn check_process_permissions(pids: &[&u32]) -> Result<(), ModelHolderError> {
    let mut permission_denied_pids = Vec::new();

    for &&pid in pids {
        let maps_path = format!("/proc/{}/maps", pid);
        match File::open(&maps_path) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                permission_denied_pids.push(pid);
            }
            Err(_) => {}
        }
    }

    if !permission_denied_pids.is_empty() {
        let current_uid = unsafe { libc::getuid() };
        if current_uid != 0 {
            eprintln!("\n=== PERMISSION ERROR ===");
            eprintln!(
                "Cannot access /proc data for processes: {:?}",
                permission_denied_pids
            );
            eprintln!("\nYou don't have permission to read process memory maps.");
            eprintln!("\nSolutions:");
            eprintln!("1. Run as root: sudo model-holder check");
            eprintln!("2. Run both llama-server and model-holder as current user");
            eprintln!("3. Ensure the processes you're checking are running as the same user");
            return Err(ModelHolderError::PermissionDenied(permission_denied_pids));
        }
    }

    Ok(())
}

/// Display process information and model files
fn display_process_info(
    process_name: &str,
    pids: &[u32],
    extensions: &HashSet<&str>,
) -> Result<HashMap<FileIdentity, (String, u64)>, ModelHolderError> {
    let mut process_models: HashMap<FileIdentity, (String, u64)> = HashMap::new();

    if pids.is_empty() {
        println!("{}: not running\n", process_name);
    } else {
        println!("=== {} processes ===\n", process_name);
        for pid in pids {
            match read_process_maps(*pid) {
                Ok(process_maps) => {
                    println!("PID {}: {}", process_maps.pid, process_maps.comm);
                    println!(
                        "  cmd: {}",
                        truncate_string(&process_maps.cmdline, MAX_CMDLINE_DISPLAY_LENGTH)
                    );

                    let aggregated = aggregate_maps(&process_maps.maps);
                    let mut model_maps: Vec<_> = aggregated
                        .iter()
                        .filter(|f| is_model_file(&f.path, extensions))
                        .collect();
                    model_maps.sort_by(|a, b| b.mapped_size.cmp(&a.mapped_size));

                    if model_maps.is_empty() {
                        println!("  (no model files mapped)");
                    } else {
                        println!("  Model files:");
                        for model_file in &model_maps {
                            println!(
                                "    {} (mapped: {}, inode: {}:{})",
                                model_file.path,
                                format_size(model_file.mapped_size),
                                model_file.identity.dev,
                                model_file.identity.inode
                            );
                            process_models.insert(
                                model_file.identity.clone(),
                                (model_file.path.clone(), model_file.mapped_size),
                            );
                        }
                    }
                }
                Err(e) => {
                    println!("PID {}: (access denied: {})", pid, e);
                }
            }
        }
        println!();
    }

    Ok(process_models)
}

/// Compare and display model file sharing between processes
fn display_model_comparison(
    holder_models: &HashMap<FileIdentity, (String, u64)>,
    llama_models: &HashMap<FileIdentity, (String, u64)>,
) {
    if holder_models.is_empty() && llama_models.is_empty() {
        return;
    }

    println!("=== Comparison (by inode) ===\n");

    let holder_ids: HashSet<_> = holder_models.keys().cloned().collect();
    let llama_ids: HashSet<_> = llama_models.keys().cloned().collect();

    let shared: Vec<_> = holder_ids.intersection(&llama_ids).collect();
    let holder_only: Vec<_> = holder_ids.difference(&llama_ids).collect();
    let llama_only: Vec<_> = llama_ids.difference(&holder_ids).collect();

    let mut size_mismatch_count = 0;

    if !shared.is_empty() {
        println!("Shared model files ({}):", shared.len());
        for file_identity in &shared {
            let holder_info = holder_models.get(file_identity).unwrap();
            let llama_info = llama_models.get(file_identity).unwrap();
            let (holder_path, holder_size) = holder_info;
            let (llama_path, llama_size) = llama_info;

            let size_ratio = if *holder_size > 0 {
                *llama_size as f64 / *holder_size as f64
            } else {
                1.0
            };

            let has_mismatch = size_ratio < (1.0 - SIZE_MISMATCH_THRESHOLD)
                || size_ratio > (1.0 + SIZE_MISMATCH_THRESHOLD);

            let status = if has_mismatch {
                size_mismatch_count += 1;
                "[INFO]"
            } else {
                "[OK]"
            };

            if holder_path == llama_path {
                println!("  {} {}", status, holder_path);
            } else {
                println!(
                    "  {} {} (holder) <-> {} (server)",
                    status, holder_path, llama_path
                );
            }
            println!("       inode {}:{}", file_identity.dev, file_identity.inode);
            println!(
                "       holder: {} | server: {}",
                format_size(*holder_size),
                format_size(*llama_size)
            );
            if has_mismatch {
                println!(
                    "       ^ server has {:.1}% mapped (will increase as pages are accessed)",
                    size_ratio * 100.0
                );
            }
        }
        println!();
    }

    if !holder_only.is_empty() {
        println!(
            "Model files in model-holder but NOT in llama-server ({}):",
            holder_only.len()
        );
        for file_identity in &holder_only {
            let info = holder_models.get(file_identity).unwrap();
            println!("  [WARN] {} (holder is pre-caching unused model)", info.0);
        }
        println!();
    }

    if !llama_only.is_empty() {
        println!(
            "Model files in llama-server but NOT in model-holder ({}):",
            llama_only.len()
        );
        for file_identity in &llama_only {
            let info = llama_models.get(file_identity).unwrap();
            println!("  [WARN] {} (not pre-cached by holder)", info.0);
        }
        println!();
    }

    if holder_only.is_empty() && llama_only.is_empty() && !shared.is_empty() {
        if size_mismatch_count == 0 {
            println!("All model files are properly shared between holder and server.");
        } else {
            println!(
                "All model files are shared. {} file(s) have partial server mappings (normal during startup).",
                size_mismatch_count
            );
        }
    }
}

/// Main check command implementation
fn check_mmaps(extensions: &str) -> Result<(), ModelHolderError> {
    validate_extensions(extensions)?;
    let ext_set: HashSet<&str> = extensions.split(',').map(|s| s.trim()).collect();

    println!("Looking for model-holder and llama-server processes...\n");

    let holder_pids = find_processes_by_name(&[MODEL_HOLDER_PROCESS_NAME])?;
    let llama_pids = find_processes_by_name(LLAMA_PROCESS_NAMES)?;

    if holder_pids.is_empty() && llama_pids.is_empty() {
        println!("No model-holder or llama-server processes found.");
        return Ok(());
    }

    let all_pids: Vec<_> = holder_pids.iter().chain(&llama_pids).collect();
    check_process_permissions(&all_pids)?;

    let holder_models = display_process_info("model-holder", &holder_pids, &ext_set)?;
    let llama_models = display_process_info("llama-server", &llama_pids, &ext_set)?;

    display_model_comparison(&holder_models, &llama_models);

    Ok(())
}

/// Truncate string to maximum length with ellipsis
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Main entry point
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    let result = match args.command {
        Some(Commands::Hold(hold_args)) => hold_models(
            hold_args.model_paths,
            hold_args.no_warmup,
            hold_args.page_size,
        )
        .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) }),
        Some(Commands::Check { extensions }) => {
            check_mmaps(&extensions).map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })
        }
        None => {
            // Legacy mode: if model_paths provided, run hold
            if !args.model_paths.is_empty() {
                hold_models(args.model_paths, args.no_warmup, args.page_size)
                    .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })
            } else {
                // No args, show help
                eprintln!("Usage: model-holder [hold] <MODEL_PATHS>...");
                eprintln!("       model-holder check");
                eprintln!("\nRun 'model-holder --help' for more information.");
                std::process::exit(1);
            }
        }
    };

    if let Err(ref error) = result {
        error!("Application error: {}", error);
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_region(path: &str, dev: &str, inode: u64, start: u64, end: u64) -> MappedRegion {
        MappedRegion {
            path: path.to_string(),
            start,
            end,
            dev: dev.to_string(),
            inode,
        }
    }

    #[test]
    fn test_file_identity_equality() {
        let id1 = FileIdentity {
            dev: "00:3c".to_string(),
            inode: 12345,
        };
        let id2 = FileIdentity {
            dev: "00:3c".to_string(),
            inode: 12345,
        };
        let id3 = FileIdentity {
            dev: "00:3c".to_string(),
            inode: 99999,
        };

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_file_identity_hash_same_file_different_paths() {
        // Same file (same dev:inode) should have same hash regardless of path
        let id1 = FileIdentity {
            dev: "00:3c".to_string(),
            inode: 36281902,
        };
        let id2 = FileIdentity {
            dev: "00:3c".to_string(),
            inode: 36281902,
        };

        let mut set: HashSet<FileIdentity> = HashSet::new();
        set.insert(id1.clone());

        // Should find id2 in the set since it's the same file
        assert!(set.contains(&id2));
    }

    #[test]
    fn test_aggregate_maps_single_region() {
        let regions = vec![make_region("/path/to/model.gguf", "00:3c", 12345, 0, 1000)];

        let aggregated = aggregate_maps(&regions);

        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].path, "/path/to/model.gguf");
        assert_eq!(aggregated[0].mapped_size, 1000);
        assert_eq!(aggregated[0].identity.inode, 12345);
    }

    #[test]
    fn test_aggregate_maps_multiple_regions_same_file() {
        // Same file mapped in multiple regions (common with mmap)
        let regions = vec![
            make_region("/path/to/model.gguf", "00:3c", 12345, 0, 1000),
            make_region("/path/to/model.gguf", "00:3c", 12345, 1000, 3000),
            make_region("/path/to/model.gguf", "00:3c", 12345, 3000, 5000),
        ];

        let aggregated = aggregate_maps(&regions);

        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].mapped_size, 1000 + 2000 + 2000); // 5000 total
    }

    #[test]
    fn test_aggregate_maps_same_file_different_paths() {
        // Same file (same inode) but different paths (e.g., bind mount)
        let regions = vec![
            make_region("/mnt/models/model.gguf", "00:3c", 12345, 0, 1000),
            make_region("/container/models/model.gguf", "00:3c", 12345, 1000, 2000),
        ];

        let aggregated = aggregate_maps(&regions);

        // Should aggregate to ONE file since same inode
        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].mapped_size, 2000);
        assert_eq!(aggregated[0].identity.inode, 12345);
    }

    #[test]
    fn test_aggregate_maps_different_files() {
        let regions = vec![
            make_region("/path/model1.gguf", "00:3c", 11111, 0, 1000),
            make_region("/path/model2.gguf", "00:3c", 22222, 0, 2000),
        ];

        let aggregated = aggregate_maps(&regions);

        assert_eq!(aggregated.len(), 2);
    }

    #[test]
    fn test_is_model_file() {
        let extensions: HashSet<&str> = ["gguf", "ggml", "bin"].into_iter().collect();

        assert!(is_model_file("/path/to/model.gguf", &extensions));
        assert!(is_model_file("/path/to/model.GGUF", &extensions)); // case insensitive
        assert!(is_model_file("/path/to/model.ggml", &extensions));
        assert!(is_model_file("/path/to/model.bin", &extensions));

        assert!(!is_model_file("/path/to/libc.so", &extensions));
        assert!(!is_model_file("/path/to/config.json", &extensions));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1536), "1.50 KB");
        assert_eq!(format_size(1_048_576), "1.00 MB");
        assert_eq!(format_size(1_073_741_824), "1.00 GB");
        assert_eq!(format_size(10_737_418_240), "10.00 GB");
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(truncate_string("exactly10!", 10), "exactly10!");
        assert_eq!(truncate_string("this is too long", 10), "this is...");
    }

    #[test]
    fn test_comparison_same_file_different_paths() {
        // Simulate holder and server having same file via different paths
        let holder_id = FileIdentity {
            dev: "00:3c".to_string(),
            inode: 36281902,
        };
        let server_id = FileIdentity {
            dev: "00:3c".to_string(),
            inode: 36281902,
        };

        let mut holder_models: HashMap<FileIdentity, (String, u64)> = HashMap::new();
        let mut server_models: HashMap<FileIdentity, (String, u64)> = HashMap::new();

        holder_models.insert(
            holder_id,
            ("/mnt/noir/models/model.gguf".to_string(), 9_000_000_000),
        );
        server_models.insert(server_id, ("/models/model.gguf".to_string(), 9_000_000_000));

        let holder_ids: HashSet<_> = holder_models.keys().cloned().collect();
        let server_ids: HashSet<_> = server_models.keys().cloned().collect();

        let shared: Vec<_> = holder_ids.intersection(&server_ids).collect();

        // Should find 1 shared file even though paths differ
        assert_eq!(shared.len(), 1);
    }

    #[test]
    fn test_comparison_detects_size_mismatch() {
        let _id = FileIdentity {
            dev: "00:3c".to_string(),
            inode: 36281902,
        };

        let holder_size: u64 = 9_000_000_000; // 9 GB
        let server_size: u64 = 440_000_000; // 440 MB

        let size_ratio = server_size as f64 / holder_size as f64;
        let has_mismatch = size_ratio < 0.9 || size_ratio > 1.1;

        // 440MB / 9GB = ~4.9%, which is < 90%, so should be flagged
        assert!(has_mismatch);
        assert!(size_ratio < 0.1); // Actually < 10%
    }

    #[test]
    fn test_comparison_no_mismatch_when_sizes_close() {
        let holder_size: u64 = 9_000_000_000;
        let server_size: u64 = 8_800_000_000; // 97.8%

        let size_ratio = server_size as f64 / holder_size as f64;
        let has_mismatch = size_ratio < 0.9 || size_ratio > 1.1;

        assert!(!has_mismatch);
    }

    #[test]
    fn test_validate_page_size_valid() {
        assert!(validate_page_size(4096).is_ok());
        assert!(validate_page_size(8192).is_ok());
        assert!(validate_page_size(1024).is_ok());
    }

    #[test]
    fn test_validate_page_size_invalid() {
        assert!(validate_page_size(0).is_err());
        assert!(validate_page_size(1000).is_err());
        assert!(validate_page_size(4097).is_err());
    }

    #[test]
    fn test_validate_extensions_valid() {
        assert!(validate_extensions("gguf,ggml,bin").is_ok());
        assert!(validate_extensions("gguf").is_ok());
        assert!(validate_extensions("GGUF,GML").is_ok());
    }

    #[test]
    fn test_validate_extensions_invalid() {
        assert!(validate_extensions("").is_err());
        assert!(validate_extensions("gguf,.ggml").is_err());
        assert!(validate_extensions("gguf,ggml,").is_err());
        assert!(validate_extensions(",gguf").is_err());
    }

    #[test]
    fn test_constants_values() {
        assert_eq!(DEFAULT_PAGE_SIZE, 4096);
        assert_eq!(PROGRESS_UPDATE_INTERVAL, 1000);
        assert_eq!(PROGRESS_UPDATE_TIMEOUT_MS, 100);
        assert!((SIZE_MISMATCH_THRESHOLD - 0.1).abs() < f64::EPSILON);
        assert_eq!(MAX_CMDLINE_DISPLAY_LENGTH, 80);
        assert_eq!(DEFAULT_EXTENSIONS, "gguf,ggml,bin");
    }

    #[test]
    fn test_process_name_constants() {
        assert_eq!(MODEL_HOLDER_PROCESS_NAME, "model-holder");
        assert_eq!(
            LLAMA_PROCESS_NAMES,
            &["llama-server", "llama-cli", "llama.cpp"]
        );
    }
}
