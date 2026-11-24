# parity: mode=c
# parity: serial=1

echo "builtin: MSYSTEM=${MSYSTEM-unset} OSTYPE=$OSTYPE"
env | grep '^MSYSTEM=' | sed 's/^/msys: /'
MSYS_NO_PATHCONV=1 cmd.exe /d /c "echo MSYSTEM=%MSYSTEM%" | tr -d '\r' | sed 's/^/cmd: /'
echo "uname: $(uname -s | cut -d_ -f1)"
