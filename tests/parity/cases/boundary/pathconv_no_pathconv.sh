# parity: mode=c
# parity: serial=1

echo "cmd sees with MSYS_NO_PATHCONV:"
MSYS_NO_PATHCONV=1 cmd.exe /d /c echo /quiet /c/x --opt=/c/x /a:/b /tmp/y x=/c/y | tr -d '\r'
