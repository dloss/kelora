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

    **Debian/Ubuntu:** download [.deb](https://github.com/dloss/kelora/releases/latest), then:
    ```bash
    sudo dpkg -i kelora_*_amd64.deb
    ```

    **Fedora/RHEL:** download [.rpm](https://github.com/dloss/kelora/releases/latest), then:
    ```bash
    sudo dnf install kelora-*.x86_64.rpm
    ```

    **ARM:** see [releases](https://github.com/dloss/kelora/releases) for aarch64 binaries.

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

    See [all releases](https://github.com/dloss/kelora/releases) for ARM Linux, FreeBSD, OpenBSD, and more.

## Shell completion

```bash
kelora --completions bash > ~/.local/share/bash-completion/completions/kelora
kelora --completions zsh > "${fpath[1]}/_kelora"
kelora --completions fish > ~/.config/fish/completions/kelora.fish
```

PowerShell and elvish are supported too (`--completions powershell`).

## Check it works

```bash
kelora --version
```

Then [explore your first log file](guide/explore.md).
