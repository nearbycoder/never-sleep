// A local illustration only: no analytics, network calls, or system controls.
const awake = document.querySelector('#awake-switch');
const display = document.querySelector('#display-switch');
const dock = document.querySelector('#dock-switch');
const power = document.querySelector('#power-preview');
const status = document.querySelector('#preview-status');
const source = document.querySelector('#preview-source');
const announcement = document.querySelector('#preview-announcement');

function renderPreview() {
  const onBattery = power.getAttribute('aria-pressed') === 'true';
  const enabled = awake.getAttribute('aria-checked') === 'true';
  const displayOn = display.getAttribute('aria-checked') === 'true';
  const active = enabled && !onBattery;
  const message = active
    ? (displayOn ? 'Keeping your Mac and display awake' : 'Keeping your Mac awake')
    : (enabled && onBattery ? 'Paused while on battery' : 'Sleep protection is off');
  status.replaceChildren(document.createElement('span'), document.createTextNode(message));
  status.classList.toggle('paused', !active);
  source.textContent = onBattery ? 'Running on battery' : 'Power adapter connected';
  power.querySelector('span').textContent = onBattery ? 'Reconnect power' : 'Unplug power';
  announcement.textContent = `Preview: ${message}. ${dock.getAttribute('aria-checked') === 'true' ? 'Dock icon is shown.' : 'Menu bar only.'}`;
}

for (const toggle of [awake, display, dock]) {
  toggle.addEventListener('click', () => {
    toggle.setAttribute('aria-checked', String(toggle.getAttribute('aria-checked') !== 'true'));
    renderPreview();
  });
}
power.addEventListener('click', () => {
  power.setAttribute('aria-pressed', String(power.getAttribute('aria-pressed') !== 'true'));
  renderPreview();
});
