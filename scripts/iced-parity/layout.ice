viewport: 1024x768
mode: Immediate
-----
expect "Centered"
expect "I am centered!"
click "Next →"
expect "Column"
click "Next →"
expect "Row"
click "← Previous"
expect "Column"
