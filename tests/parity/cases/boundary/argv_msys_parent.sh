# gash started by an MSYS program reads its args from the command line the MSYS runtime built.

echo "timeout:"
/usr/bin/timeout 5 bash -c "printf '%s\n' 'a\\\\b'"
echo "awkward args:"
printf 'EXPANDED\n' > x
/usr/bin/timeout 5 bash -c 'printf "[%s]\n" "$@"' x 'a\\b' 'c\d' 'q"r' "it's" '' '@x' '*' 'sp ace' 'tail\\' '{a,b}'
rm -f x
