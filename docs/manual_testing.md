# Things to Test Include (but not limited to):

## Text channel interactions

- /help
- r!help
- /ping
- r!ping
- /uptime
- r!uptime

## All of these these need to be tried with bot being in/out of vc, target in/out of vc, you in/out of vc (use alts for targets probably)

- ban
    
    - you in vc, target in vc:
    - you in vc, target not in vc:
    - you not in vc, target in vc:
    - you not in vc, target not in vc:
    
- mute
    
    - you in vc, target in vc:
    - you in vc, target not in vc:
    - you not in vc, target in vc:
    - you not in vc, target not in vc:
	
- unmute
    
    - you in vc, target in vc:
    - you in vc, target not in vc:
    - you not in vc, target in vc:
    - you not in vc, target not in vc:
	
- kick
    
    - you in vc, target in vc:
    - you in vc, target not in vc:
    - you not in vc, target in vc:
    - you not in vc, target not in vc:
    
- timeout
    
    - you in vc, target in vc:
    - you in vc, target not in vc:
    - you not in vc, target in vc:
    - you not in vc, target not in vc:

## More tests that need to be run.
## Open question, should the default be that everyone can play music?

- create voice channel:
- delete voice channel:
- create text channel:
- delete text channel:

  ## Music Command Tests
  Make note next to any command where something goes wrong.
- Test 1
	- r!summon
	- /leave
	- /join
	- r!fuck off
	- r!summon
	- r!vol (should be 100)
	- r!p <spotify_link>
	- r!vol (should be 100) 
	- r!seek 1:00 (make sure it's playing)
	- r!seek 0:00 (make sure it's playing)
	- r!pause
	- r!p <youtube link> (should not unpause)
	- r!q (should be two songs in queue)
	- r!resume
	- r!skip
	- r!skip (should autoplay)
	- r!downvote (should skip and autoplay another song)
	- r!p <Name of a Song>
	- r!now_playing
	- r!np
	- r!playlog
	- r!stop
	- r!leave
	- r!summon
	- r!p <Name of a Song>
	- r!vol (should be 100)
- create playlist:
- add to playlist:
- rename playlist:
- show play log:
