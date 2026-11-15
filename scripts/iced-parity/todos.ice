viewport: 500x800
mode: Immediate
preset: Empty
-----
click #new-task
type "Write the parity harness"
type enter
type "Replay it under rawr"
type enter
type "Compare the results"
type enter
expect "3 tasks left"
click "Write the parity harness"
expect "2 tasks left"
click "Replay it under rawr"
expect "1 task left"
