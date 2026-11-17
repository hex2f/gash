# A script found through PATH gets the path it was found at as $0 (PATH entry + file name),
# so `dirname "$0"` is its folder, as the npm/pnpm shims in node_modules/.bin rely on.
t=$(mktemp -d)
cd "$t" || exit 1
mkdir -p sub/node_modules/.bin sub/node_modules/pkg/bin
cd sub || exit 1

printf '#!/bin/sh\necho "0=$0"\necho "dirname=$(dirname "$0")"\necho "source=${BASH_SOURCE[0]}"\n' > node_modules/.bin/probe
# The shape of the POSIX shims npm, pnpm and yarn write.
printf '#!/bin/sh\nbasedir=$(dirname "$(echo "$0" | sed -e '"'"'s,\\\\,/,g'"'"')")\nexec sh "$basedir/../pkg/bin/run.sh" "$@"\n' > node_modules/.bin/shim
printf 'echo "pkg ran: $*"\n' > node_modules/pkg/bin/run.sh
chmod +x node_modules/.bin/probe node_modules/.bin/shim

# Absolute paths differ per machine: show them relative to the temp dir.
show() { sed -e "s,$t,<t>,g"; }

echo "-- relative entry"
(PATH="./node_modules/.bin:$PATH"; probe)
echo "-- relative entry, no ./"
(PATH="node_modules/.bin:$PATH"; probe)
echo "-- relative entry, trailing slash"
(PATH="./node_modules/.bin/:$PATH"; probe)
echo "-- absolute entry"
(PATH="$t/sub/node_modules/.bin:$PATH"; probe) | show
echo "-- empty entry (current directory)"
(cd node_modules/.bin && PATH=":$PATH" && probe)
echo "-- PATH given as a prefix assignment"
PATH="./node_modules/.bin:$PATH" probe
echo "-- in a pipeline and in the background"
(PATH="./node_modules/.bin:$PATH"; probe | cat; probe & wait)
echo "-- npm shim, run from elsewhere"
(PATH="./node_modules/.bin:$PATH"; shim a b)
(PATH="$t/sub/node_modules/.bin:$PATH"; cd / && shim c d)
echo "-- typed with a slash: as typed"
./node_modules/.bin/probe
node_modules/.bin/probe
echo "-- run as bash script: as given"
cp node_modules/.bin/probe given.sh
bash given.sh

cd / && rm -rf "$t"
