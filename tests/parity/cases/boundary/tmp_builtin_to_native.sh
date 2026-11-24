# parity: mode=c
# parity: serial=1

echo builtin-wrote > /tmp/p_b2n.txt
echo "cmd type: $(MSYS_NO_PATHCONV=1 cmd.exe /d /c "type %TEMP%\p_b2n.txt" 2>&1 | tr -d '\r')"
echo "ps get: $(powershell -NoProfile -Command 'Get-Content (Join-Path $env:TMP p_b2n.txt)' 2>&1 | tr -d '\r')"
MSYS_NO_PATHCONV=1 cmd.exe /d /c "echo cmd-wrote> %TEMP%\p_n2b.txt"
if [ -f /tmp/p_n2b.txt ]; then
  IFS= read -r line < /tmp/p_n2b.txt
  printf 'read: %s\n' "${line%$'\r'}"
else
  echo "read: /tmp/p_n2b.txt missing"
fi
