# parity: mode=c
# parity: serial=1

echo copied > "$PARITY_PROBE_DIR/src.txt"
cp "$PARITY_PROBE_DIR/src.txt" /tmp/p_m2b_cp.txt
touch /tmp/p_m2b_touch
read -r line < /tmp/p_m2b_cp.txt
echo "read cp: $line"
[ -f /tmp/p_m2b_touch ] && echo "test -f touch: yes" || echo "test -f touch: no"
for f in /tmp/p_m2b*; do echo "glob: $f"; done
echo builtin-wrote > /tmp/p_b2m.txt
echo "cat: $(cat /tmp/p_b2m.txt 2>&1)"
