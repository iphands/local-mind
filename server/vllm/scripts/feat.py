import torch, importlib, triton, os, subprocess

def check(mod):
    try:
        m = importlib.import_module(mod)
        return f"OK ({getattr(m, '__file__', 'no-file')})"
    except Exception as e:
        return f"MISSING ({e})"

print("\n=== GPU / CUDA ===")
print("GPU:", torch.cuda.get_device_name(0))
print("Cap:", torch.cuda.get_device_capability(0))
print("CUDA:", torch.version.cuda)
print("Arch list:", torch.cuda.get_arch_list())

print("\n=== CORE STACK ===")
print("torch:", torch.__version__)
print("triton:", triton.__version__)
print("flashinfer:", check("flashinfer"))
print("vllm:", check("vllm"))
print("vllm._C:", check("vllm._C"))

print("\n=== ENV SIGNALS ===")
print("TRITON_CACHE_DIR:", os.environ.get("TRITON_CACHE_DIR"))
print("TORCH_COMPILE_CACHE:", os.environ.get("TORCH_COMPILE_CACHE"))

print("\n=== QUICK KERNEL HINTS ===")
mods = ["flashinfer", "vllm", "triton"]
for m in mods:
    try:
        __import__(m)
        print(f"{m}: import OK")
    except:
        print(f"{m}: import FAIL")

print("\n=== CUDA DEVICES ===")
props = torch.cuda.get_device_properties(0)
print("SM count:", props.multi_processor_count)
print("VRAM GB:", round(props.total_memory / 1e9, 2))

print("\n=== HEURISTIC FLAGS ===")

arch = torch.cuda.get_arch_list()
print("SM120 supported:", any("120" in x for x in arch))

flashinfer_ok = False
try:
    import flashinfer
    flashinfer_ok = True
except:
    pass

print("FlashInfer present:", flashinfer_ok)

vllm_c_ok = False
try:
    import vllm._C
    vllm_c_ok = True
except:
    pass

print("vLLM compiled ext:", vllm_c_ok)

print("\n=== LIKELY STATUS ===")

if not flashinfer_ok:
    print("❌ FlashInfer missing → likely SDPA fallback")
if not vllm_c_ok:
    print("❌ vLLM C++ extensions missing → Python fallback path")
if not any("120" in x for x in arch):
    print("❌ No SM120 in arch list → non-Blackwell kernels")

if flashinfer_ok and vllm_c_ok and any("120" in x for x in arch):
    print("✅ Stack looks complete — performance issue is likely runtime kernel selection (MoE/KV/attention)")
