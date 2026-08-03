.build_version macos, 15, 0
.text
start:
    cbz x0, done
    b tail
done:
    ret
tail:
    ret
