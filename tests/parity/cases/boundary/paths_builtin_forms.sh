# parity: mode=c
# parity: serial=1
# parity: fixture=native

m=$(cygpath -m "$PWD")
w=$(cygpath -w "$PWD")
for p in "$PWD/a.txt" "$m/a.txt" "$w\\a.txt"; do
  [ -f "$p" ] && echo "test -f: yes" || echo "test -f: no"
  read -r line < "$p" && echo "read: $line"
done
cd "$w" && echo "cd win: $(pwd)"
cd "$m" && echo "cd mixed: $(pwd)"
cd "$(cygpath -uW)" && echo "cd posix: $(pwd)"
