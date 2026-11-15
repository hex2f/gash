# parity: mode=lc
# parity: serial=1

echo "builtin: TMP=$TMP TEMP=$TEMP"
echo builtin-wrote > /tmp/p_lc.txt
echo "msys cat: $(cat /tmp/p_lc.txt 2>&1)"
echo "cmd type: $(MSYS_NO_PATHCONV=1 cmd.exe /d /c "type %TEMP%\p_lc.txt" 2>&1 | tr -d '\r')"
MSYS_NO_PATHCONV=1 cmd.exe /d /c "echo cmd-wrote> %TMP%\p_lc_cmd.txt" 2>&1 | tr -d '\r' | sed 's/^/cmd write: /'
echo "msys cat back: $(cat /tmp/p_lc_cmd.txt 2>&1 | tr -d '\r')"
