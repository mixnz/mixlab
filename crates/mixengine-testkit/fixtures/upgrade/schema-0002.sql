-- What `schema-0002.db` was seeded with — roadmap task T89's fixtures, at the schema v0.0.16 ships.
--
-- The first schema after the fold into `0001_initial.sql`: `0002_service_last_failure.sql` adds
-- `services.last_failure_json` (T200b), and v0.0.16 is the first release that writes it. Captured at
-- that release so the release after it has a database of v0.0.16's shape to upgrade from.
--
-- The rows are `schema-0001.sql`'s, so the two fixtures differ by the one thing the migration
-- changed: the last statement puts `mongodb@main` in `failed` with the note T200b writes beside that
-- state, `{"at", "reason", "detail"}` as `mixengine_proto::ServiceFailureNote` serialises it.
--
-- Every table carries rows, and every closed vocabulary shows more than one of its words: the
-- runtime kinds, `stopped_by` in each of its three words, a redirecting site and a route of each
-- target. `home.id` is not seeded: the migration writes it during the capture.
--
-- **Every row is one this build can read**, because a daemon reads all of them: Mailpit's
-- `manifest_json` is `crates/mixengine-testkit/fixtures/extensions/mailpit.toml` as
-- `extensions::manifest::to_value` renders it, the pending operation is a `PrivilegedOp` as
-- `elevation::enqueue` encodes it, and PHP's two extension columns are what the index and
-- `runtimes::extensions` write. A placeholder in any of them stops the daemon rendering anything.
-- What is still not real is every path on disk, so a daemon started on this reports the programs
-- it cannot find — which is the machine, not the data.
--
-- **Two things this seed deliberately does not carry**, because a real `mixengined` starts on this
-- fixture in `crates/mixengine-cli/tests/upgrade.rs` and a fixture should not hand it work it
-- cannot do. No service is `running` with a pid: the supervisor adopts a survivor by pid *and*
-- start time, and a pid literal in a committed file names whatever process happens to hold it on
-- the machine reading it. And no site is shared: sharing is a bind on a LAN address and a firewall
-- rule, which is not something a fixture should provoke.
--
-- Nothing in it is real. Paths are a developer's home that does not exist, hashes are not hashes,
-- and every URL is under `example.invalid`, so a test that accidentally acted on a row would fail
-- rather than touch anything.

INSERT INTO runtime_installs
    (id, kind, version, channel, install_path, installed_at, size_bytes, source_url, sha256,
     is_default, provides_json, extension_dir, extensions_json, extension_choices_json)
VALUES
    (1, 'php',  '8.3.12', 'stable', '/home/dev/MixEngine/runtimes/php/8.3.12',
     '2026-01-04T09:00:00Z', 82000000, 'https://example.invalid/php-8.3.12.tar.zst', 'aa01', 1,
     '{"php":"bin/php","php-fpm":"sbin/php-fpm"}',
     '/home/dev/MixEngine/runtimes/php/8.3.12/lib/php/extensions',
     '{"static":["opcache"],"shared":["redis","xdebug"],"enabled":["redis"]}',
     '{"xdebug":true}'),
    (2, 'php',  '8.2.20', 'stable', '/home/dev/MixEngine/runtimes/php/8.2.20',
     '2026-01-04T09:05:00Z', 80000000, 'https://example.invalid/php-8.2.20.tar.zst', 'aa02', 0,
     '{"php":"bin/php"}', '', '{}', '{}'),
    (3, 'node', '22.8.0', 'stable', '/home/dev/MixEngine/runtimes/node/22.8.0',
     '2026-01-04T09:10:00Z', 51000000, 'https://example.invalid/node-22.8.0.tar.zst', 'aa03', 1,
     '{"node":"bin/node","npm":"bin/npm"}', '', '{}', '{}'),
    (4, 'composer', '2.8.4', 'stable', '/home/dev/MixEngine/runtimes/composer/2.8.4',
     '2026-01-04T09:11:00Z', 3000000, 'https://example.invalid/composer-2.8.4.phar', 'aa04', 1,
     '{"composer":"composer.phar"}', '', '{}', '{}'),
    (5, 'go',   '1.25.1', 'stable', '/home/dev/MixEngine/runtimes/go/1.25.1',
     '2026-01-04T09:12:00Z', 230000000, 'https://example.invalid/go-1.25.1.tar.zst', 'aa05', 1,
     '{"go":"bin/go","gofmt":"bin/gofmt"}', '', '{}', '{}'),
    (6, 'java', '21.0.8', 'stable', '/home/dev/MixEngine/runtimes/java/21.0.8',
     '2026-01-04T09:13:00Z', 190000000, 'https://example.invalid/java-21.0.8.tar.zst', 'aa06', 1,
     '{"java":"bin/java","javac":"bin/javac"}', '', '{}', '{}');

INSERT INTO packages
    (id, name, version, install_path, installed_at, source_url, sha256, size_bytes, provides_json)
VALUES
    (1, 'mariadb', '11.4.2', '/home/dev/MixEngine/packages/mariadb/11.4.2',
     '2026-01-04T09:15:00Z', 'https://example.invalid/mariadb-11.4.2.tar.zst', 'bb01', 340000000,
     '{"mariadbd":"bin/mariadbd"}'),
    (2, 'caddy',   '2.11.4', '/home/dev/MixEngine/packages/caddy/2.11.4',
     '2026-01-04T09:16:00Z', 'https://example.invalid/caddy-2.11.4.tar.zst',   'bb02',  48000000,
     '{"caddy":"caddy"}'),
    (3, 'mongodb', '8.0.4',  '/home/dev/MixEngine/packages/mongodb/8.0.4',
     '2026-01-04T09:17:00Z', 'https://example.invalid/mongodb-8.0.4.tar.zst',  'bb03', 420000000,
     '{"mongod":"bin/mongod"}');

-- Before the services that name it: `services` has a foreign key into this table.
INSERT INTO extensions
    (id, name, version, kind, manifest_json, install_dir, data_dir, source, signed, installed_at)
VALUES
    ('mailpit', 'Mailpit', '1.31.0', 'service',
     '{"artifact":{"linux-aarch64":{"sha256":"db3e685ed59d58354a29a4e7bfd0497f050af771a41e122aabdb0bd4fb915952","size":9821099,"url":"https://github.com/axllent/mailpit/releases/download/v1.31.0/mailpit-linux-arm64.tar.gz"},"linux-x86_64":{"sha256":"076b5ded9a2182842b93e761b9586a1a251445bffe2666f9f22a6dc14470237d","size":10634648,"url":"https://github.com/axllent/mailpit/releases/download/v1.31.0/mailpit-linux-amd64.tar.gz"},"macos-aarch64":{"sha256":"108c4d8345368825924a61c492d96ffd82961f84cda5137c8e1ed03c1d2433b7","size":10092536,"url":"https://github.com/axllent/mailpit/releases/download/v1.31.0/mailpit-darwin-arm64.tar.gz"},"macos-x86_64":{"sha256":"a1be2868e0b596664c4479b4539b08dc69b51224444a09a3f607801f6dc54319","size":10684568,"url":"https://github.com/axllent/mailpit/releases/download/v1.31.0/mailpit-darwin-amd64.tar.gz"},"windows-aarch64":{"sha256":"af0013c31253befb73dd1974891d37921de6454563d83074d27d443c40f2f0ce","size":9855752,"url":"https://github.com/axllent/mailpit/releases/download/v1.31.0/mailpit-windows-arm64.zip"},"windows-x86_64":{"sha256":"39b5f1b9a671a5f1738e3792e8495bef2fec8176b02ee1e22635626c5888c6e3","size":10768592,"url":"https://github.com/axllent/mailpit/releases/download/v1.31.0/mailpit-windows-amd64.zip"}},"extension":{"description":"Local SMTP capture and web UI","homepage":"https://mailpit.axllent.org","id":"mailpit","kind":"service","name":"Mailpit","version":"1.31.0"},"permissions":{"filesystem":["own-data"],"network":"loopback","services":[]},"ports":{"smtp_port":1025,"ui_port":8025},"recipe":{"front_end":[],"php_ini":[{"key":"sendmail_path","value":"{install_dir}/mailpit sendmail --smtp-addr {listen}:{smtp_port}"}]},"schema":1,"service":{"args":["--listen","{listen}:{ui_port}","--smtp","{listen}:{smtp_port}","--db-file","{data_dir}/mailpit.db"],"cwd":"{data_dir}","env":{"TZ":{"from":"literal","value":"UTC"}},"health":null,"program":"{install_dir}/mailpit","ready":{"addr":"{listen}:{ui_port}","timeout":10000,"type":"tcp"},"reload":null,"restart":{"backoff":{"initial":500,"jitter_percent":20,"max":30000,"multiplier_percent":200},"max_retries":5,"type":"on_failure","window":300000},"stop":{"grace":10000,"type":"signal"}}}',
     '/home/dev/MixEngine/extensions/mailpit',
     '/home/dev/MixEngine/data/extensions/mailpit', 'registry', 1, '2026-01-04T09:50:00Z');

INSERT INTO extension_ports (extension_id, name, port)
VALUES ('mailpit', 'ui_port', 8025), ('mailpit', 'smtp_port', 1025);

-- `stopped_by` in each of its three words, and every one of them stopped.
INSERT INTO services
    (id, package_id, runtime_install_id, extension_id, instance_name, state, autostart, port,
     activation_port, bind_addr, data_dir, config_overrides_json, limits_json, idle_minutes,
     stopped_by, last_started_at, last_exit_code, pid, pid_start_time)
VALUES
    ('mariadb@main', 1, NULL, NULL, 'main', 'stopped', 1, 3306, 13306, '127.0.0.1',
     '/home/dev/MixEngine/data/mariadb/main', '{"innodb_buffer_pool_size":"256M"}', '{}', 30,
     'daemon', 1767517200000, 0, NULL, NULL),
    ('mariadb@legacy', 1, NULL, NULL, 'legacy', 'stopped', 0, 3307, NULL, '127.0.0.1',
     '/home/dev/MixEngine/data/mariadb/legacy', '{}', '{}', NULL, 'person',
     1767517210000, 0, NULL, NULL),
    ('caddy@main',   2, NULL, NULL, 'main', 'stopped', 1,   80, NULL, '127.0.0.1',
     NULL, '{}', '{}', NULL, 'daemon', 1767517260000, 0, NULL, NULL),
    ('mongodb@main', 3, NULL, NULL, 'main', 'stopped', 0, 27017, NULL, '127.0.0.1',
     '/home/dev/MixEngine/data/mongodb/main', '{}', '{}', NULL, 'never',
     NULL, NULL, NULL, NULL),
    ('php-fpm@8.3',  NULL, 1, NULL, '8.3',  'stopped', 0, 9000, 19000, '127.0.0.1',
     NULL, '{}', '{"memory_mb":512}', 10, 'never', NULL, NULL, NULL, NULL),
    ('mailpit@main', NULL, NULL, 'mailpit', 'main', 'stopped', 1, 8025, NULL, '127.0.0.1',
     '/home/dev/MixEngine/data/extensions/mailpit', '{}', '{}', NULL, 'daemon',
     1767517280000, 0, NULL, NULL);

INSERT INTO blueprints
    (id, name, description, manifest_toml, created_at, source, trusted, signature)
VALUES
    ('laravel',       'Laravel',       'A Laravel project', 'schema = 1',
     '2026-01-04T09:20:00Z', 'builtin',  1, NULL),
    ('my-shop',       'My shop',       '',                  'schema = 1',
     '2026-01-04T09:21:00Z', 'captured', 1, NULL),
    ('from-a-friend', 'From a friend', '',                  'schema = 1',
     '2026-01-04T09:22:00Z', 'imported', 0, NULL);

INSERT INTO projects
    (id, name, root_path, runtime_pins_json, created_at, blueprint_id, keep_warm)
VALUES
    (1, 'blog', '/home/dev/blog', '{"php":"8.3.12"}', '2026-01-04T09:30:00Z', 'laravel', 1),
    (2, 'shop', '/home/dev/shop', '{"go":"1.25.1","java":"21.0.8"}',
     '2026-01-04T09:31:00Z', NULL, 0);

INSERT INTO sites
    (id, project_id, extension_id, doc_root, kind, php_service_id, https_enabled, https_redirect,
     http_port, https_port, config_json, state, shared_interface, shared_address, shared_since,
     shared_until)
VALUES
    (1, 1, NULL, '/home/dev/blog/public', 'php-fpm', 'php-fpm@8.3', 1, 1, 80, 443, '{}', 'enabled',
     NULL, NULL, NULL, NULL),
    (2, 2, NULL, '/home/dev/shop/public', 'static',  NULL,          0, 0, 80, 443, '{}', 'disabled',
     NULL, NULL, NULL, NULL);

-- One route of each target, and `position` deliberately not in path order.
INSERT INTO site_routes (id, site_id, position, path, target, php_service_id, config_json)
VALUES
    (1, 1, 0, '/api',        'proxy',   NULL,          '{"upstream":"127.0.0.1:3000"}'),
    (2, 1, 1, '/legacy',     'php-fpm', 'php-fpm@8.3', '{}'),
    (3, 2, 0, '/assets',     'static',  NULL,          '{"root":"/home/dev/shop/dist"}');

INSERT INTO bin_commands (name, kind)
VALUES ('yarn', 'node'), ('pnpm', 'node'), ('laravel', 'composer');

INSERT INTO site_domains (id, site_id, domain, is_primary)
VALUES (1, 1, 'blog.test', 1), (2, 1, 'www.blog.test', 0), (3, 2, 'shop.test', 1);

INSERT INTO site_service_links (site_id, service_id)
VALUES (1, 'mariadb@main'), (2, 'mongodb@main');

INSERT INTO ca (id, fingerprint, cert_path, key_path, created_at, installed_in_trust_store)
VALUES (1, 'ab:cd:ef:01', '/home/dev/MixEngine/certs/ca/ca.crt',
        '/home/dev/MixEngine/certs/ca/ca.key', '2026-01-04T09:40:00Z', 1);

INSERT INTO certificates
    (id, domain, sans_json, not_before, not_after, cert_path, key_path,
     issued_by_ca_fingerprint, revoked)
VALUES
    (1, 'blog.test', '["blog.test","www.blog.test"]', '2026-01-04T09:41:00Z',
     '2027-01-04T09:41:00Z', '/home/dev/MixEngine/certs/blog.test.crt',
     '/home/dev/MixEngine/certs/blog.test.key', 'ab:cd:ef:01', 0);

INSERT INTO jobs (id, kind, state, percent, message, started_at, finished_at, result_json)
VALUES
    (1, 'runtime.install', 'succeeded', 100, 'php 8.3.12 installed',
     1767516000000, 1767516120000, '{"version":"8.3.12"}'),
    (2, 'cert.issue',      'running',    40, 'issuing blog.test',
     1767517000000, NULL, NULL);

INSERT INTO events (id, ts, kind, subject, payload_json)
VALUES
    (1, '2026-01-04T09:45:00Z', 'site.created',    'blog.test',  '{}'),
    (2, '2026-01-04T09:46:00Z', 'service.started', 'caddy@main', '{"pid":4242}');

INSERT INTO settings (key, value_json)
VALUES ('telemetry', 'false'), ('update.channel', '"stable"');

INSERT INTO pending_privileged_ops (id, op, dedupe_key, requested_at)
VALUES (1, '{"op":"hosts-apply","entries":[{"address":"127.0.0.1","domain":"blog.test"}]}',
        'hosts-apply', 1767517400000);

INSERT INTO metrics_minutes (subject, minute, cpu_avg, cpu_peak, rss_avg, rss_peak, samples)
VALUES ('caddy@main',   29458620, 0.4, 1.2, 41943040, 46137344, 12),
       ('mariadb@main', 29458620, 1.1, 3.7, 268435456, 289406976, 12);

-- The column `0002_service_last_failure.sql` adds, holding what a build of v0.0.16 writes into it.
UPDATE services
SET state = 'failed',
    stopped_by = 'never',
    last_failure_json =
        '{"at":1767517300000,"reason":{"kind":"spawn_failed"},"detail":"the process could not be started at all"}'
WHERE id = 'mongodb@main';
