# parity: mode=c
# parity: serial=1

export TMP=/tmp TEMP=/tmp
echo "builtin: TMP=$TMP TEMP=$TEMP"
env | grep -E '^(TMP|TEMP)=' | sort | sed 's/^/msys: /'
MSYS_NO_PATHCONV=1 cmd.exe /d /c "echo TMP=%TMP% TEMP=%TEMP%" | tr -d '\r' | sed 's/^/cmd: /'
MSYS_NO_PATHCONV=1 cmd.exe /d /c "echo cmd-wrote> %TEMP%\p_px.txt" 2>&1 | tr -d '\r' | sed 's/^/cmd write: /'
echo "msys cat: $(cat /tmp/p_px.txt 2>&1 | tr -d '\r')"
