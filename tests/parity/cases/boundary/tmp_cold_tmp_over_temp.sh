# parity: mode=c
# parity: serial=1
# parity: cold=1
# parity: env=TMP=@CASE@\fixture

echo "cygpath -w /tmp: $(cygpath -w /tmp)"
echo builtin-wrote > /tmp/p_order.txt
echo "msys cat /tmp: $(cat /tmp/p_order.txt 2>&1)"
echo "native: $(MSYS_NO_PATHCONV=1 cmd.exe /d /c "if exist %TMP%\p_order.txt (echo sees it) else (echo misses it)" | tr -d '\r')"
