#!/bin/fish
# Check if an argument is provided
if test -z "$argv[1]"
    echo "Usage: push.fish <branch>"
    exit 1
end

set branch $argv[1]

# Push to Github.
git push origin $branch
# No no-verify for sr.ht because they don't support lfs.
git push --no-verify sr.ht $branch
