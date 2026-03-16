I want to start working on a plan ... See the ./run* files here
I want to make a new file... ./run-qwen-experiment

My goal is to run: ./run-qwen-experiment /mnt/noir/scratch/ai/llm/models/vllm/Qwen3.5-122B-A10B-NVFP4
AND:
- NOT OOM
- Get the maximum TPS with 262144 context window (by setting --profile-high)
- Get the maximum TPS with 131072 context window (by setting --profile-low)

My hardware is a SINGLE RTX Pro 6000 Blackwell (96GB)

We should also be able to use `--benchmark --profile-high` and `--benchmark --profile-high`
To run a benchmark with this prompt
```
Write a Bash program that calculates primes from 1 - 10_000_00 (dont use external programs, Pure bash)
After writing the Bash program write an ANSI C primes calculator too
Store both programs in ./scratch/<timestamp>/

We are going to need to do extensive web crawling looking at examples, documentation, etc
- https://docs.vllm.ai/projects/recipes/en/latest/Qwen/Qwen3.5.html
- https://github.com/vllm-project/vllm/issues
- https://huggingface.co/Sehyo/Qwen3.5-122B-A10B-NVFP4
- https://huggingface.co/unsloth/Qwen3.5-122B-A10B

Also the source code for VLLM is in ./vendor we should use it!

IMPORTANT! Make sure you write down all discoveries made while looking at ./vendor AND from web searches to ./context/ (in markdown files)

Put the plan in ./context/000_vllm_runner.md
