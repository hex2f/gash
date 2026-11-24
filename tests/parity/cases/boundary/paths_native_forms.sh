# parity: mode=c
# parity: serial=1
# parity: fixture=native

m=$(cygpath -m "$PWD")
w=$(cygpath -w "$PWD")
echo "cmd type posix: $(cmd.exe //d //c type "$PWD/a.txt" 2>&1 | tr -d '\r')"
echo "cmd type mixed: $(cmd.exe //d //c type "$m/a.txt" 2>&1 | tr -d '\r')"
echo "cmd type win: $(cmd.exe //d //c type "$w\\a.txt" 2>&1 | tr -d '\r')"
