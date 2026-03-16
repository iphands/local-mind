## Setup

- GPU: Single RTX Pro 6000 Blackwell (96GB vram)
- Model runner: VLLM docker.io/vllm/vllm-openai:cu130-nightly
- Model: Qwen3.5-122B-A10B-NVFP4

NOTE: We will also test on Qwen3.5-35B-A3B-FP8 because its way faster to start and thus debug

## Goal

To get an optimal sglang config for our GPU and Model
We want to optimize for tokens per second at 128k context length or higher
We prefer the 262144 context length

## Process

### Initial

We execute ./run-qwen /mnt/noir/scratch/ai/llm/models/vllm/Qwen3.5-122B-A10B-NVFP4 2>&1 | tee server.log
To run and save logs

### Tests 1

Once this works we will fire up claude client and start saying things like
- Hi
- Hello
- How are you

### Tests 2

Once Tests 1 works we will ask
- Write a program to calculate primes up to 10_000_00 in pure Bash
- Write a program to calculate primes up to 10_000_00 in ANSI C
