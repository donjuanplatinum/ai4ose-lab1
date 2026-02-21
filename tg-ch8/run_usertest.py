import pexpect
import sys
import os

print("Spawning qemu for ch8b_usertest...")
# Use a larger buffer and encoding
child = pexpect.spawn('cargo run', encoding='utf-8', timeout=600)
child.logfile = sys.stdout

try:
    child.expect('Rust user shell\r\n>> ', timeout=120)
    print("Shell detected, sending ch8b_usertest...")
    child.sendline('ch8b_usertest')
    
    # Expect each test case or final message
    index = child.expect(['Shell: Process 2 exited with code 0', 'Panic', pexpect.TIMEOUT, pexpect.EOF], timeout=300)
    
    if index == 0:
        print("\n[SUCCESS] ch8b_usertest completed successfully.")
    elif index == 1:
        print("\n[FAILURE] Kernel or user space PANIC detected.")
    elif index == 2:
        print("\n[TIMEOUT] ch8b_usertest timed out.")
    else:
        print("\n[EOF] QEMU exited unexpectedly.")
        
except Exception as e:
    print(f"\n[ERROR] {str(e)}")
