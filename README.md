# Booster 

Booster is a minimal terminal agent focused on speed, simplicity, and developer enjoyment.

## Setup

Booster requires an Anthropic API key to function. Set the following environment variable before running the utility:

```sh
export ANTHROPIC_API_KEY=your_api_key_here
```

You can also add this to your shell profile (e.g. `~/.bashrc`, `~/.zshrc`) to make it permanent:

```sh
echo 'export ANTHROPIC_API_KEY=your_api_key_here' >> ~/.zshrc
```

Booster will exit with an error if `ANTHROPIC_API_KEY` is not set.

