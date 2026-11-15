# parity: mode=host
# parity: timeout=30

cat
echo "cat rc=$?"
if read -r x; then echo "read got [$x]"; else echo "read eof rc=$?"; fi
echo "head bytes: $(head -c 5 | wc -c)"
