# parity: mode=c
# parity: serial=1
# parity: cold=1
# parity: env=TMP=@CASE@\fixture
# parity: env=TEMP=@CASE@\fixture

echo "builtin TMP: $TMP"
echo "cygpath -w /tmp: $(cygpath -w /tmp)"
echo builtin-wrote > /tmp/p_csd.txt
echo "msys cat /tmp: $(cat /tmp/p_csd.txt 2>&1)"
echo "native: $(MSYS_NO_PATHCONV=1 cmd.exe /d /c "if exist %TEMP%\p_csd.txt (echo sees it) else (echo misses it)" | tr -d '\r')"
