"use strict";
let csrf;
let groups = [];
const element = (id) => document.getElementById(id);
async function request(url, options = {}) {
  const response = await fetch(url, {cache: "no-store", signal: AbortSignal.timeout(20000), ...options});
  if (!response.ok) throw new Error(await response.text());
  return response;
}
function showGrants(grants) {
  element("grants").replaceChildren(...grants.map(({ip, group, expires}) => {
    const row = document.createElement("tr");
    for (const value of [groups.find((item) => item.id === group)?.label || group, ip, new Date(expires * 1000).toLocaleString()]) {
      const cell = document.createElement("td");
      cell.textContent = value;
      row.append(cell);
    }
    return row;
  }));
}
async function visit() {
  element("retry").disabled = true;
  element("error").textContent = "";
  try {
    const statusResponse = await fetch("/api/status", {cache: "no-store", signal: AbortSignal.timeout(20000)});
    if (statusResponse.status === 401) {
      element("login").hidden = false;
      element("account").hidden = true;
      return;
    }
    if (!statusResponse.ok) throw new Error(await statusResponse.text());
    const status = await statusResponse.json();
    csrf = status.csrf;
    groups = status.groups;
    element("games").textContent = "Your roles enable: " + groups.filter((group) => group.eligible).map((group) => group.label + (group.addressFamilies.length === 1 ? " (IPv" + group.addressFamilies[0] + " only)" : "")).join(", ") + ". Sign in again after receiving a new role.";
    element("login").hidden = true;
    element("account").hidden = false;
    element("session").textContent = "This browser is remembered until " + new Date(status.sessionExpires * 1000).toLocaleString() + ".";
    showGrants(status.grants);
    const tickets = await (await request("/api/visit", {method: "POST", headers: {"X-CSRF-Token": csrf}})).json();
    const results = await Promise.all(tickets.map(async ({family, url, ticket}) => {
      try {
        const result = await (await request(url, {method: "POST", credentials: "omit",
          headers: {"Content-Type": "application/json"}, body: JSON.stringify({ticket})})).json();
        const enabled = result.enabled.map((id) => groups.find((g) => g.id === id)?.label || id);
        const errors = result.errors.map((e) => e.groups.map((id) => groups.find((g) => g.id === id)?.label || id).join(", ") + ": " + e.message);
        return "IPv" + family + " (" + result.ip + "): " + (enabled.length ? "enabled for " + enabled.join(", ") + ". " : "No games enabled. ") + errors.join(" ");
      } catch (error) {
        return "IPv" + family + ": could not enable access. " + (error instanceof TypeError ? "This network may not support it." : error.message);
      }
    }));
    element("status").textContent = results.join(" ");
    const updated = await (await request("/api/status")).json();
    showGrants(updated.grants);
    element("error").textContent = updated.errors.map((e) => "Could not list grants for " + e.groups.map((id) => groups.find((g) => g.id === id)?.label || id).join(", ") + ".").join(" ");
  } catch (error) {
    element("error").textContent = error.message;
    element("status").textContent = "Access renewal was not completed.";
  } finally {
    element("retry").disabled = false;
  }
}
element("retry").addEventListener("click", visit);
element("logout").addEventListener("click", async () => {
  try {
    await request("/api/logout", {method: "POST", headers: {"X-CSRF-Token": csrf}});
    location.reload();
  } catch (error) { element("error").textContent = error.message; }
});
visit();
