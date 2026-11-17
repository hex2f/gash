# parity: mode=host
# parity: console=none

echo hidden
MSYS_NO_PATHCONV=1 cmd.exe /d /c echo native-in-hidden | tr -d '\r'
echo "msys: $(uname -s | cut -d_ -f1)"
echo "ps: $(powershell -NoProfile -Command 'Write-Output ps-in-hidden' | tr -d '\r')"
