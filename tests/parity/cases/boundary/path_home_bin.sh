# parity: mode=c
# parity: serial=1
# parity: home=home-dotfiles

export PATH="$HOME/bin:$PATH"
echo "command -v: $(command -v parity-hello)"
parity-hello
echo "msys env lookup: $(env parity-hello 2>&1)"
