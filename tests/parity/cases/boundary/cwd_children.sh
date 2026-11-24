# parity: mode=c
# parity: serial=1

mkdir -p 'sub dir'
cd 'sub dir'
echo "builtin pwd: $(pwd)"
echo "msys pwd: $(/usr/bin/pwd)"
echo "cmd cd: $(MSYS_NO_PATHCONV=1 cmd.exe /d /c cd | tr -d '\r')"
echo "PWD env msys: $(env | grep '^PWD=')"
