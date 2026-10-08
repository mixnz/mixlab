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

function escape(text) {
  return String(text).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

app.get("/", async (_request, response) => {
  let mongo;
  try {
    await client.db().command({ ping: 1 });
    mongo = `connected to <code>${escape(uri)}</code>`;
  } catch (error) {
    mongo = `not reachable at <code>${escape(uri)}</code>: ${escape(error.message)}`;
  }
  response.type("html").send(`<!doctype html>
<meta charset="utf-8">
<title>Express + MongoDB</title>
<h1>Hello from Express</h1>
<p>MongoDB: ${mongo}</p>
<p>Edit <code>index.js</code> and restart <code>npm start</code>.</p>`);
});

app.listen(port, "127.0.0.1", () => {
  console.log(`Listening on http://127.0.0.1:${port}, MongoDB at ${uri}`);
});
