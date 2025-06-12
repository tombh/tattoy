+++
title = "Downloading Tattoy"
template = "page.html"
+++

# Downloading Tattoy v{{ include(path='build-vars/version') }}

Tattoy has prebuilt binaries for both x86 and ARM architectures on all the 3 major OSs; Linux, MacOS and Windows. They are available on Tattoy's <a href="https://github.com/tattoy-org/tattoy/releases/tag/v{{ include(path='build-vars/version') }}">latest GitHub Releases page</a>.

## Distro Packages

### Arch Linux AUR
* `yay -S tattoy`
* `paru -S tattoy`

### Ubuntu, Debian
```sh
curl -LO https://github.com/tattoy-org/tattoy/releases/download/v{{ include(path='build-vars/version') }}/tattoy-v{{ include(path='build-vars/version') }}.deb
sudo dpkg --install tattoy-v{{ include(path='build-vars/version') }}.deb
```

### Fedora, RHEL
```sh
sudo dnf install https://github.com/tattoy-org/tattoy/releases/download/v{{ include(path='build-vars/version') }}/tattoy-v{{ include(path='build-vars/version') }}.x86_64.rpm
```

### Homebrew
`brew install tattoy-org/tap/tattoy`

## Compiling From Source

You will first need [Rust](https://www.rust-lang.org/tools/install). Then you can run: 

`cargo install --locked --git https://github.com/tattoy-org/tattoy tattoy`

Note that on Linux you may also need some development dependencies. For example on `apt`-based systems
you can install them with: `sudo apt-get install libxcb1-dev libdbus-1-dev`.

