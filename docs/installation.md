# Installation

=== "macOS"

    ```bash
    brew install dloss/kelora/kelora
    ```

    Or download a signed binary: [Apple Silicon](https://github.com/dloss/kelora/releases/latest/download/kelora-aarch64-apple-darwin.tar.gz) | [Intel](https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-apple-darwin.tar.gz)

=== "Linux"

    **Binary:**
    ```bash
    curl -LO https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-unknown-linux-musl.tar.gz
    tar xzf kelora-x86_64-unknown-linux-musl.tar.gz
    sudo mv kelora /usr/local/bin/
    ```

    **Debian/Ubuntu** (also `kelora_arm64.deb`, `kelora_armhf.deb`):
    ```bash
    curl -LO https://github.com/dloss/kelora/releases/latest/download/kelora_amd64.deb
    sudo apt install ./kelora_amd64.deb
    ```

    **Fedora/RHEL** (also `kelora-aarch64.rpm`, `kelora-armv7hl.rpm`):
    ```bash
    curl -LO https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64.rpm
    sudo dnf install ./kelora-x86_64.rpm
    ```

    ARM binaries: `kelora-aarch64-unknown-linux-musl.tar.gz`, `kelora-armv7-unknown-linux-musleabihf.tar.gz`.

=== "Windows"

    Download [kelora-x86_64-pc-windows-msvc.zip](https://github.com/dloss/kelora/releases/latest/download/kelora-x86_64-pc-windows-msvc.zip), extract, and add to PATH.

=== "Cargo"

    From [crates.io](https://crates.io/crates/kelora):
    ```bash
    cargo install kelora
    ```

    From source (latest, unreleased code):
    ```bash
    git clone https://github.com/dloss/kelora
    cd kelora
    cargo install --path . --locked
    ```

=== "Other"

    See [all releases](https://github.com/dloss/kelora/releases) for FreeBSD and OpenBSD.

## Shell completion

```bash
kelora --completions bash > ~/.local/share/bash-completion/completions/kelora
mkdir -p ~/.zfunc && kelora --completions zsh > ~/.zfunc/_kelora   # add fpath+=~/.zfunc to ~/.zshrc
kelora --completions fish > ~/.config/fish/completions/kelora.fish
```

PowerShell and elvish are supported too (`--completions powershell`).

## Check it works

```bash
kelora --version
```

Then [explore your first log file](guide/explore.md).
