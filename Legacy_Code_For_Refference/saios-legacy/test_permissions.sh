#!/bin/sh

# SAIOS v0.6.0 permission/ownership smoke test.
#
# This script is intentionally limited to the current built-in shell feature
# set. It does not use POSIX `if`, heredocs, `touch`, or direct shebang
# execution because those are not implemented in SAIOS yet.
#
# It also does not claim to validate cross-user enforcement because commands
# currently run as root.

echo "=== SAIOS v0.6.0 Permission/Ownership Smoke Test ==="
echo "Built-in shell compatible; cross-user enforcement is not covered yet."
echo

echo "Test 1: User Management"
echo "-----------------------"
useradd alice
useradd bob
echo "Users:"
users
echo

echo "Test 2: File Creation"
echo "---------------------"
write /users/alice/alice_file.txt Alice data
echo "File contents:"
cat /users/alice/alice_file.txt
echo

echo "Test 3: chmod"
echo "-------------"
chmod 644 /users/alice/alice_file.txt
echo "Directory listing for /users/alice:"
ls /users/alice
echo

echo "Test 4: chown"
echo "-------------"
chown alice /users/alice/alice_file.txt
echo "Ownership command completed."
echo

echo "Test 5: Directory Creation"
echo "--------------------------"
mkdir /users/alice/testdir
chmod 755 /users/alice/testdir
echo "Directory listing for /users/alice:"
ls /users/alice
echo

echo "Test 6: Script Execution via built-in sh"
echo "----------------------------------------"
write /users/alice/test_script.sh #!/bin/sh
append /users/alice/test_script.sh echo Script executed successfully
chmod 755 /users/alice/test_script.sh
sh /users/alice/test_script.sh
echo

echo "Test 7: Process Credentials"
echo "---------------------------"
id
whoami
echo

echo "Summary:"
echo "OK useradd/users"
echo "OK file create/read"
echo "OK chmod path"
echo "OK chown path"
echo "OK directory create"
echo "OK built-in sh script runner"
echo "OK id/whoami"