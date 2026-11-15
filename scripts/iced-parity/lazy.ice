viewport: 1024x768
mode: Immediate
-----
expect "Toggle Order (Ascending)"
expect "Foo"
click "Toggle Order (Ascending)"
expect "Toggle Order (Descending)"
click "Toggle Order (Descending)"
expect "Toggle Order (Ascending)"
