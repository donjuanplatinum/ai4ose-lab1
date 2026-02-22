import pexpect
import sys

print("Spawning qemu for Doom...")
child = pexpect.spawn('cargo run', encoding='utf-8', timeout=120)
child.logfile = sys.stdout

child.expect('Rust user shell\r\n>> ')
print("Shell detected, sending doom...")
child.sendline("doom")

child.expect('doomgeneric_tick')
# Just sleep so it stays open or logs
print("Doom is ticketing!")
child.expect(pexpect.EOF, timeout=10)
