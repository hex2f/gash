# parity: serial=1

# Over 32767 characters of command line.
args=()
for i in $(seq 1 6000); do args+=("arg-$i"); done
args+=('a b' 'say "hi"' 'q\"' 'end\' '' '@x' $'two\nlines' '*' "it's" 'é')
echo "msys child:"
/usr/bin/printf '[%s]\n' "${args[@]}" | wc -l
/usr/bin/printf '[%s]\n' "${args[@]}" | tail -n 11
echo "grep over many files:"
printf 'needle\n' > hay.txt
files=()
for i in $(seq 1 3000); do files+=(./hay.txt); done
grep -c needle "${files[@]}" | sort | uniq -c
echo "background:"
/usr/bin/printf '%s\n' "${args[@]}" > bg.out &
wait
wc -l < bg.out
echo "arguments naming files after @ stay literal:"
touch f
printf 'from-file\n' > g
/usr/bin/printf '[%s]\n' @f @g @missing x@g
env printf '[%s]\n' @g
