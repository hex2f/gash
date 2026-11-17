# A drive-letter argument holding a quote, a newline or a trailing backslash reaches an MSYS program as one word.

/usr/bin/printf '[%s]\n' 'C:\x\" -la' next
/usr/bin/printf '[%s]\n' 'C:\Program Files\' next
/usr/bin/printf '[%s]\n' $'C:\\file\n-la' next
