<?php
// A plain PHP page: what MixEngine's php-mysql blueprint starts you with. Replace it with your own.
//
// It does not connect to MySQL: the password MixEngine made for this project's account is shown
// in MixLab (and by `mix`), never written into a file.

$pdo = extension_loaded('pdo_mysql');
$mysqli = extension_loaded('mysqli');
$state = fn (bool $on) => $on ? 'dot' : 'dot warn';
$word = fn (bool $on) => $on ? 'enabled' : 'not enabled';
?>
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>PHP with MySQL</title>
<style>
:root {
  color-scheme: light dark;
  --bg: #f4f5fb;
  --glow: #dfe3ff;
  --card: #ffffff;
  --text: #1c1f2e;
  --muted: #5d6378;
  --line: #e6e8f1;
  --accent: #5b5bf0;
  --code: #f1f2f8;
  --ok: #17a34a;
  --warn: #d97706;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #11131b;
    --glow: #23264a;
    --card: #1a1d29;
    --text: #e9ebf5;
    --muted: #9aa0b8;
    --line: #2a2e3f;
    --accent: #8b8cff;
    --code: #232738;
    --ok: #4ade80;
    --warn: #fbbf24;
  }
}
* { box-sizing: border-box; }
body {
  margin: 0;
  min-height: 100vh;
  display: grid;
  place-items: center;
  padding: 32px 16px;
  font: 15px/1.6 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  color: var(--text);
  background: radial-gradient(circle at 50% 0%, var(--glow), var(--bg) 60%);
}
main {
  width: 100%;
  max-width: 560px;
  background: var(--card);
  border: 1px solid var(--line);
  border-radius: 16px;
  padding: 32px;
  box-shadow: 0 12px 40px rgb(20 20 60 / 0.08);
}
.badge {
  display: inline-block;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  padding: 4px 10px;
  border-radius: 999px;
}
h1 { margin: 14px 0 6px; font-size: 28px; line-height: 36px; }
.lead { margin: 0 0 24px; color: var(--muted); }
dl { margin: 0 0 24px; border-top: 1px solid var(--line); }
dl div {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  padding: 10px 0;
  border-bottom: 1px solid var(--line);
}
dt { color: var(--muted); }
dd { margin: 0; text-align: right; overflow-wrap: anywhere; }
.dot::before {
  content: "";
  display: inline-block;
  width: 8px;
  height: 8px;
  margin-right: 8px;
  border-radius: 50%;
  background: var(--ok);
  vertical-align: 1px;
}
.dot.warn::before { background: var(--warn); }
h2 { margin: 0 0 8px; font-size: 15px; line-height: 24px; }
ol { margin: 0; padding-left: 20px; }
li + li { margin-top: 4px; }
code {
  font: 13px/20px ui-monospace, "Cascadia Code", Consolas, monospace;
  background: var(--code);
  padding: 1px 6px;
  border-radius: 6px;
}
pre {
  margin: 8px 0 0;
  padding: 12px 14px;
  background: var(--code);
  border-radius: 10px;
  overflow-x: auto;
}
pre code { padding: 0; background: none; }
footer { margin-top: 24px; font-size: 13px; color: var(--muted); }
</style>
</head>
<body>
<main>
<span class="badge">MixEngine blueprint</span>
<h1>PHP with MySQL</h1>
<p class="lead">Plain PHP on PHP-FPM, with MySQL beside it. No framework.</p>
<dl>
<div><dt>PHP</dt><dd class="dot"><?= htmlspecialchars(PHP_VERSION) ?></dd></div>
<div><dt>pdo_mysql</dt><dd class="<?= $state($pdo) ?>"><?= $word($pdo) ?></dd></div>
<div><dt>mysqli</dt><dd class="<?= $state($mysqli) ?>"><?= $word($mysqli) ?></dd></div>
<div><dt>MySQL</dt><dd><code>127.0.0.1:3306</code></dd></div>
</dl>
<h2>Next</h2>
<ol>
<li>Edit <code>index.php</code> in the project folder, and reload.</li>
<li>Find the database, account and password in MixLab, on this project.</li>
<li>Connect with PDO:
<pre><code>$db = new PDO(
    'mysql:host=127.0.0.1;port=3306;dbname=&lt;database&gt;',
    '&lt;account&gt;',
    '&lt;password&gt;'
);</code></pre></li>
</ol>
<footer>A starter from MixEngine&rsquo;s blueprint. Replace it with your own; nothing else depends on it.</footer>
</main>
</body>
</html>
