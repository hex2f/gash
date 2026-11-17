# parity: mode=c
# parity: serial=1
# parity: fixture=native

s=$(cygpath -w "$PWD/printargs.ps1")
MSYS_NO_PATHCONV=1 powershell -NoProfile -ExecutionPolicy Bypass -File "$s" 'a b' plain 'end\' 'q\"x' '' '*' | tr -d '\r'
