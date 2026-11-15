# parity: mode=c
# parity: serial=1

echo "builtin: TMP=$TMP TEMP=$TEMP TMPDIR=${TMPDIR-unset}"
env | grep -E '^(TMP|TEMP|TMPDIR)=' | sort | sed 's/^/msys: /'
MSYS_NO_PATHCONV=1 cmd.exe /d /c "echo TMP=%TMP% TEMP=%TEMP% TMPDIR=%TMPDIR%" | tr -d '\r' | sed 's/^/cmd: /'
