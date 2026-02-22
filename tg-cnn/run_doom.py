import pexpect
import sys

child = pexpect.spawn('cargo run', encoding='utf-8', timeout=60)
child.logfile = sys.stdout
try:
    child.expect('Rust user shell\r\n>> ')
    child.sendline('doom')
    child.expect(pexpect.EOF, timeout=10)
except pexpect.TIMEOUT:
    pass
