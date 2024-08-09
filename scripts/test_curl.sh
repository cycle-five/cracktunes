curl --verbose 127.0.0.1:3030/dbl/webhook \
  -H 'Authorization: asdfasdf' \
  -H 'Content-Type: Application/JSON' \
  -d '{"bot": "10", "user": "10", "type": "test", "isWeekend": false, "query": "test"}'
