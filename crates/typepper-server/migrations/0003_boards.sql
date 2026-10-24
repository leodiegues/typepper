-- What the boards read: each player's best per mode, all time or since a
-- period began (docs/online.md, "Boards").
create index runs_board  on runs (mode, player_id, score desc) where player_id is not null and score is not null;
create index runs_period on runs (mode, created_at)            where player_id is not null and score is not null;
