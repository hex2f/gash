# parity: mode=c
# parity: serial=1
# parity: fixture=native

m=$(cygpath -m "$PWD")
w=$(cygpath -w "$PWD")
echo "posix: $(cat "$PWD/a.txt" 2>&1)"
echo "mixed: $(cat "$m/a.txt" 2>&1)"
echo "win: $(cat "$w\\a.txt" 2>&1)"
