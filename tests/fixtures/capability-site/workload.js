// Test fixture only: independent web behavior, with no Neurobrowser integration.
window.workloadTrace = [];
const trace = (kind, detail) => workloadTrace.push({ kind, detail });
function advance() {
  trace('advance', 'shipment dispatched');
  document.querySelector('#shipment-fact').textContent = 'Shipment state: dispatched.';
}
document.querySelector('#advance').addEventListener('click', advance);
document.querySelector('#replace').addEventListener('click', () => {
  const prior = document.querySelector('#advance');
  const replacement = document.createElement('button');
  replacement.id = 'advance';
  replacement.type = 'button';
  replacement.textContent = 'Advance replacement shipment';
  replacement.addEventListener('click', advance);
  prior.replaceWith(replacement);
  trace('replace', 'button identity changed');
});
document.querySelector('#spa').addEventListener('click', () => {
  history.pushState({}, '', '/route/north?view=live');
  document.title = 'North route live manifest';
  trace('spa', location.pathname);
});
for (const name of ['input', 'change']) {
  document.querySelector('#recipient').addEventListener(name, event => trace(name, event.target.value));
}
document.querySelector('#dispatch-form').addEventListener('submit', event => {
  event.preventDefault();
  const recipient = document.querySelector('#recipient').value;
  document.querySelector('#form-fact').textContent = `Form state: dispatched to ${recipient}.`;
  trace('submit', recipient);
  trace('submitter', { id: event.submitter?.id || null, destination: event.submitter?.formAction || event.target.action });
});
fetch('/facts.json')
  .then(response => { if (!response.ok) throw new Error('fixture fetch failed'); return response.json(); })
  .then(facts => {
    document.querySelector('#loaded-fact').textContent = facts.fact;
    trace('fetch', facts.fact);
    window.workloadReady = true;
  });
