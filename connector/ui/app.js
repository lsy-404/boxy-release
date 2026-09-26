const connectForm = document.querySelector('#connect-form');
const setup = document.querySelector('#setup');
const status = document.querySelector('#status');
const error = document.querySelector('#error');
const remote = document.querySelector('#remote');
const statusTitle = document.querySelector('#status-title');
const statusMessage = document.querySelector('#status-message');
const statusError = document.querySelector('#status-error');
const statusDot = document.querySelector('#status-dot');
const editorButton = document.querySelector('#open-editor');
const stopButton = document.querySelector('#stop');

document.documentElement.dataset.fluentTheme = matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
document.querySelector('#theme').addEventListener('click', () => {
  const next = document.documentElement.dataset.fluentTheme === 'dark' ? 'light' : 'dark';
  document.documentElement.dataset.fluentTheme = next;
});

function send(action, fields = {}) {
  window.ipc.postMessage(JSON.stringify({ action, ...fields }));
}

connectForm.addEventListener('submit', event => {
  event.preventDefault();
  error.hidden = true;
  send('connect', { url: remote.value.trim() });
});
editorButton.addEventListener('click', () => send('openEditor'));
stopButton.addEventListener('click', () => send('stop'));

window.boxyUpdate = update => {
  switch (update.kind) {
    case 'error':
      if (setup.hidden) {
        statusError.textContent = update.message;
        statusError.hidden = false;
      } else {
        error.textContent = update.message;
        error.hidden = false;
      }
      break;
    case 'connected':
      setup.hidden = true;
      status.hidden = false;
      document.querySelector('#remote-label').textContent = update.remote;
      break;
    case 'status':
      statusMessage.textContent = update.message;
      break;
    case 'editorReady':
      editorButton.disabled = false;
      statusTitle.textContent = 'Connected';
      statusDot.classList.add('ready');
      break;
    case 'finished':
      statusTitle.textContent = 'Complete';
      statusMessage.textContent = update.message;
      statusDot.classList.add('ready');
      stopButton.textContent = 'Close';
      break;
    case 'failed':
      statusTitle.textContent = 'Connection failed';
      statusMessage.textContent = update.message;
      statusDot.classList.add('failed');
      stopButton.textContent = 'Close';
      break;
  }
};
