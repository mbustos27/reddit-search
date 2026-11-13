# Test fixtures

`arctic/` holds responses in the exact shape the Arctic Shift archive returns them, recorded once and then edited:

- every post and comment text was replaced with invented text;
- usernames were replaced with `user_N`;
- bulky fields the tool never reads were dropped.

Ids, subreddit names, timestamps and field names are as recorded, so the parser is tested against the real shape. Nothing here is a real person's writing.
