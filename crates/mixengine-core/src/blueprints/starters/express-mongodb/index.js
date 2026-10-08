// An Express server that talks to MongoDB: what MixEngine's express-mongodb blueprint starts you
// with. Replace it with your own; `npm start` runs whatever `main` says.
//
// PORT is the port MixEngine's site forwards to (3000 unless you changed the site), and
// MONGODB_URI the database. Without one, it is the MongoDB MixEngine runs on this machine, with a
// database named after this folder.

const path = require("node:path");
const express = require("express");
const { MongoClient } = require("mongodb");

const port = Number(process.env.PORT ?? 3000);
const database = path.basename(process.cwd()).replace(/[^A-Za-z0-9_-]/g, "_");
const uri = process.env.MONGODB_URI ?? `mongodb://127.0.0.1:27017/${database}`;

const client = new MongoClient(uri, { serverSelectionTimeoutMS: 2000 });
const app = express();

// Files in public/ are served as they are: this page's stylesheet, and anything you add.
app.use(express.static(path.join(__dirname, "public")));

function escape(text) {
  return String(text).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

app.get("/", async (_request, response) => {
  let mongo;
  try {
    await client.db().command({ ping: 1 });
    mongo = `<dd class="dot">connected</dd>`;
  } catch (error) {
    mongo = `<dd class="dot warn">not reachable: ${escape(error.message)}</dd>`;
  }
  response.type("html").send(`<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Express + MongoDB</title>
<link rel="stylesheet" href="/style.css">
</head>
<body>
<main>
<span class="badge">MixEngine blueprint</span>
<h1>Express + MongoDB</h1>
<p class="lead">A Node.js server, with MongoDB beside it.</p>
<dl>
<div><dt>Node.js</dt><dd class="dot">${escape(process.version)}</dd></div>
<div><dt>Express</dt><dd class="dot">listening on port ${port}</dd></div>
<div><dt>MongoDB</dt>${mongo}</div>
<div><dt>Database</dt><dd><code>${escape(uri)}</code></dd></div>
</dl>
<h2>Next</h2>
<ol>
<li>Edit <code>index.js</code> in the project folder.</li>
<li>Stop <code>npm start</code> with Ctrl+C and run it again.</li>
<li>Set <code>MONGODB_URI</code> first if your MongoDB is not on port 27017.</li>
</ol>
<footer>A starter from MixEngine&rsquo;s blueprint. Replace it with your own; nothing else depends on it.</footer>
</main>
</body>
</html>`);
});

app.listen(port, "127.0.0.1", () => {
  console.log(`Listening on http://127.0.0.1:${port}, MongoDB at ${uri}`);
});
