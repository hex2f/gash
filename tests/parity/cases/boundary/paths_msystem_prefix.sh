# parity: mode=c

p=/${MSYSTEM,,}
echo "builtin: $([ -d "$p/bin" ] && echo dir || echo missing)"
echo "native: $(cmd.exe //d //c echo "$p/bin" | tr -d '\r')"
