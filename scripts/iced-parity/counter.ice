viewport: 300x300
mode: Immediate
-----
expect "0"
click "Increment"
click "Increment"
click "Increment"
click "Decrement"
expect "2"
