viewport: 1024x768
mode: Immediate
-----
expect "Example Toast"
expect "Add more toasts in the form below!"
expect "Add Toast"
click "X"
expect "Title"
type tab
type "Parity"
type tab
type "A toast that expires"
type enter
expect "A toast that expires"
