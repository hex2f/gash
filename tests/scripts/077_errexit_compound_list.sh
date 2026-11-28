# Under `set -e`, a compound command whose status comes from a failure errexit ignores (a
# non-final command of an && or || list, a `!` pipeline) does not exit the shell. A subshell
# or function call with that status does, as do a plain failure and a failing final command.
t=$(mktemp -d)
cd "$t" || exit 1

try() {
  out=$("$BASH" -c "set -e; $1; echo after" 2>/dev/null)
  echo "rc=$? [$out] $1"
}

for fail in '[ a = b ] && :' 'false || false && :'; do
  try "for i in 1 2; do $fail; done"
  try "i=0; while [ \$i -lt 2 ]; do i=\$((i + 1)); $fail; done"
  try "i=0; until [ \$i -ge 2 ]; do i=\$((i + 1)); $fail; done"
  try "{ $fail; }"
  try "( $fail )"
  try "f() { $fail; }; f"
  try "if true; then $fail; fi"
  try "case a in a) $fail ;; esac"
  try "for ((i = 0; i < 1; i++)); do $fail; done"
  try "for i in 1; do for j in 1; do $fail; done; done"
  try "f() { { $fail; }; echo in-f; }; f"
  try "trap 'echo ERR' ERR; { $fail; }"
done
try 'for i in 1; do ! true; done'
try "trap 'echo ERR' ERR; if true; then ! true; fi"

echo "-- still exits"
try 'for i in 1; do false; done'
try 'true && false'
try '{ :; } > /nonexistent_dir_077/x'
try 'for i in 1; do :; done > /nonexistent_dir_077/x'
try "trap 'echo ERR' ERR; for i in 1; do false; done"
try "trap 'echo ERR \$?' ERR; true; { :; } > /nonexistent_dir_077/x"
try "trap 'echo ERR \$?' ERR; true; for i in 1; do :; done > /nonexistent_dir_077/x"

echo "-- ERR fires once per failure without set -e"
echo "[$("$BASH" -c 'trap "echo ERR" ERR; { false; }; for i in 1; do false; done; if true; then false; fi; echo after' 2>/dev/null | tr '\n' ' ')]"

# Git Bash does not wait for a process substitution at exit, so tee can still be writing.
settle() {
  i=0
  while [ $i -lt 50 ] && ! { grep -qx end out 2>/dev/null && grep -qx end log 2>/dev/null; }; do
    sleep 0.1
    i=$((i + 1))
  done
}

echo "-- script with its output through tee"
printf '%s\n' 'set -euo pipefail' 'exec > >(tee log) 2>&1' 'last=7' \
  'for taken in 1 3 5 12 75; do [ "$last" = "$taken" ] && exit 9; done' \
  '[ a = b ] && exit 9' 'echo probe' 'echo end' > s.sh
"$BASH" s.sh > out 2>&1
echo "rc=$?"
settle
echo "out:"
cat out
echo "log:"
cat log

cd / && rm -rf "$t"
