# vllm Runner Design: run-qwen-experiment

## Hardware

- **GPU**: RTX Pro 6000 Blackwell — 96GB VRAM
- **Single card** (no NVLink/SLI required for this model)

## Model

**Qwen3.5-122B-A10B-NVFP4**

- MoE architecture: 122B total parameters, ~10B active per forward pass
- Quantization: NVFP4 (4-bit NV format) — requires Blackwell (SM 100+) hardware
- Expert parallel supported via `--enable-expert-parallel`

## Memory Math

| Item | Estimate |
|------|----------|
| NVFP4 weights (122B × 0.5 bytes) | ~61GB |
| Scales / overhead | ~4–7GB |
| **Total weight footprint** | **~65–68GB** |
| GPU total | 96GB |
| `--gpu-memory-utilization 0.95` → usable | ~91.2GB |
| Available for KV cache | **~23–26GB** |

**KV cache per token (FP8, 48 layers):**
```
48 layers × 2 (K+V) × 8 kv_heads × 128 head_dim × 1 byte = 98,304 bytes/token
```

**Total KV pool:** ~24GB / 98,304 ≈ 245K tokens

- At 2K tokens/seq (benchmark prompt): supports ~122 concurrent seqs
- At 131072 context/seq: fits ~1–2 seqs fully loaded
- At 262144 context/seq: edge case, 1 seq

`--max-num-seqs 96` is set conservatively below the theoretical max for stability.

## Profiles

Both profiles use identical LLAMA_ARGS except `--max-model-len`:

| Setting | profile-low | profile-high |
|---------|-------------|--------------|
| `--max-model-len` | 131072 | 262144 |
| `--max-num-seqs` | 96 | 96 |
| `--gpu-memory-utilization` | 0.95 | 0.95 |

**profile-low** is the default (used when no `--profile-*` flag is given).

**Trade-off**: profile-high doubles the maximum context window but doesn't change TPS meaningfully for short requests. For long-document workloads, profile-high is needed. For chat/tool-use, profile-low wastes less KV reservation.

## Key vllm Args

| Arg | Reason |
|-----|--------|
| `--attention-backend flashinfer` | Blackwell-optimized attention kernel |
| `--async-scheduling` | Decouples prefill/decode scheduling for higher throughput |
| `--enable-chunked-prefill` | Splits long prefills across steps — critical for 131K/262K context without OOM |
| `--enable-expert-parallel` | MoE-specific: distributes expert layers across CUDA streams on one GPU |
| `--kv-cache-dtype fp8` | Halves KV cache memory vs bf16 |
| `--language-model-only` | Skips vision encoder init — saves ~2–3GB |
| `--dtype auto` | Picks bf16 for non-quantized layers automatically |
| `--swap-space 0` | No CPU swap — all in GPU VRAM |
| `--reasoning-parser qwen3` | Enables `<think>` block extraction from responses |
| `--tool-call-parser qwen3_coder` | Tool call format for Qwen3 Coder variants |
| `--trust-remote-code` | Required for custom model architectures |

## Container

```
docker.io/vllm/vllm-openai:cu130-nightly
```

The `:latest` tag is also assigned first (pattern from `run-qwen`), with `:cu130-nightly` taking effect as the final assignment. `cu130-nightly` is required for NVFP4 support on Blackwell (CUDA 13.0).

Port mapping: host `8700` → container `8799`.

## Served Model Names

```
cosmo-proxy cosmo-6000 claude-haiku-4-5-20251001
```

Matches `run-qwen` exactly — clients connecting to this server use any of these aliases.

## Benchmark Workflow

The `--benchmark` flag makes the script fully self-contained:

1. Creates `./scratch/<timestamp>/` on the host
2. Writes a JSONL dataset (`bench_prompts.jsonl`) with one custom prompt
3. Starts server in detached mode (`docker run -d`)
4. Polls `http://localhost:8700/health` every 5s for up to 10 minutes
5. Sends two warmup requests sequentially (ensures torch-compile cache is warm)
6. Runs `vllm bench serve` in a separate container with `--network host`
7. Stops the server, prints results

**Benchmark prompt** (targets long-output generation for realistic TPS):
> Write a Bash program that calculates primes from 1–10,000,000 (pure bash, no external programs). After writing the Bash program, write an ANSI C primes calculator too.

**Custom JSONL format** (from `vendor/vllm/vllm/benchmarks/datasets.py`):
Each line must have a `"prompt"` key. The dataset wraps to fill `--num-prompts 50`.

**`--num-prompts 50`**: Enough requests for a statistically stable TPS reading; the single prompt is cycled.

## Usage

```bash
# Server mode (defaults to profile-low)
./run-qwen-experiment /mnt/.../Qwen3.5-122B-A10B-NVFP4
./run-qwen-experiment /mnt/.../Qwen3.5-122B-A10B-NVFP4 --profile-low
./run-qwen-experiment /mnt/.../Qwen3.5-122B-A10B-NVFP4 --profile-high

# Self-contained benchmark
./run-qwen-experiment /mnt/.../Qwen3.5-122B-A10B-NVFP4 --benchmark --profile-low
./run-qwen-experiment /mnt/.../Qwen3.5-122B-A10B-NVFP4 --benchmark --profile-high
```

## Verification

```bash
# Manual health check after server start
curl http://localhost:8700/health
curl http://localhost:8700/v1/models

# Check docker logs if server hangs
docker logs vllm

# Benchmark results
ls -lh ./scratch/
cat ./scratch/<timestamp>/result_low_<timestamp>.json
```
