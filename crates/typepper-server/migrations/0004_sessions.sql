-- Web login: a browser is a player while its cookie's secret matches a row
-- here (docs/built/server.md, "Web login"). Only the secret's hash is kept,
-- so the table alone can't sign anyone in.
create table sessions (
  hash       bytea  primary key,             -- sha256 of the cookie's secret
  player_id  bigint not null references players on delete cascade,
  created_at timestamptz not null default now()
);
