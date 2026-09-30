import type { Snippet } from "./snippets";

/**
 * The snippet set shipped with the app.
 *
 * **Constants in code, not going through the store.** The consequences are right both ways:
 * upgrading MixLab updates this set along with it, and what users write themselves is never
 * overwritten by an upgrade.
 *
 * Built-in snippets cannot be edited or deleted. Wanting a different version means adding a
 * snippet of your own — far simpler than building the notion of "a built-in that has been edited",
 * which would have to answer "what if an upgrade changes this snippet" with no pleasant answer.
 *
 * Every entry has to pass the module's tool admission criterion: it sits on the path of a dev
 * working with a DB, an API or a server. Quotes are placed where needed — `-p'{{password}}'` —
 * because `fill` deliberately does not wrap anything.
 */
export const BUILTIN: Snippet[] = [
  {
    id: "b-mysqldump",
    title: "mysqldump",
    group: "mysql",
    template:
      "mysqldump -h {{host}} -P {{port}} -u {{user}} -p'{{password}}' {{database}} > {{file}}.sql",
  },
  {
    id: "b-mysql-restore",
    title: "mysql restore",
    group: "mysql",
    template:
      "mysql -h {{host}} -P {{port}} -u {{user}} -p'{{password}}' {{database}} < {{file}}.sql",
  },
  {
    id: "b-pgdump",
    title: "pg_dump",
    group: "postgres",
    template: "pg_dump -h {{host}} -p {{port}} -U {{user}} -Fc {{database}} > {{file}}.dump",
  },
  {
    id: "b-pgrestore",
    title: "pg_restore",
    group: "postgres",
    template:
      "pg_restore -h {{host}} -p {{port}} -U {{user}} -d {{database}} --clean --if-exists {{file}}.dump",
  },
  {
    id: "b-psql",
    title: "psql",
    group: "postgres",
    template: "psql -h {{host}} -p {{port}} -U {{user}} -d {{database}}",
  },
  {
    id: "b-mongodump",
    title: "mongodump",
    group: "mongo",
    template: "mongodump --uri='{{uri}}' --db={{database}} --out={{dir}}",
  },
  {
    id: "b-mongorestore",
    title: "mongorestore",
    group: "mongo",
    template: "mongorestore --uri='{{uri}}' --db={{database}} {{dir}}/{{database}}",
  },
  {
    id: "b-redis-cli",
    title: "redis-cli",
    group: "redis",
    template: "redis-cli -h {{host}} -p {{port}} -a '{{password}}' -n {{db}}",
  },
  {
    id: "b-docker-mysql",
    title: "docker run mysql",
    group: "docker",
    template:
      "docker run -d --name {{name}} -e MYSQL_ROOT_PASSWORD='{{password}}' -p {{port}}:3306 mysql:8",
  },
  {
    id: "b-docker-postgres",
    title: "docker run postgres",
    group: "docker",
    template:
      "docker run -d --name {{name}} -e POSTGRES_PASSWORD='{{password}}' -p {{port}}:5432 postgres:16",
  },
  {
    id: "b-docker-logs",
    title: "docker logs",
    group: "docker",
    template: "docker logs -f --tail 200 {{container}}",
  },
  {
    id: "b-docker-prune",
    title: "docker system prune",
    group: "docker",
    template: "docker system prune -af --volumes",
  },
  {
    id: "b-systemctl",
    title: "systemctl status",
    group: "server",
    template: "systemctl status {{service}}",
  },
  {
    id: "b-journalctl",
    title: "journalctl",
    group: "server",
    template: "journalctl -u {{service}} -n 200 --no-pager",
  },
  {
    id: "b-ssh-tunnel",
    title: "SSH tunnel",
    group: "ssh",
    template: "ssh -N -L {{local_port}}:{{remote_host}}:{{remote_port}} {{user}}@{{server}}",
  },
  {
    id: "b-scp",
    title: "scp",
    group: "ssh",
    template: "scp {{user}}@{{server}}:{{remote_path}} {{local_path}}",
  },
];
