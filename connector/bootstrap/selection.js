const form = document.getElementById('service-form');
const input = document.getElementById('service-url');
const button = document.getElementById('connect');
const error = document.getElementById('error');

form.addEventListener('submit', async (event) => {
  event.preventDefault();
  button.disabled = true;
  input.disabled = true;
  error.textContent = '';
  button.textContent = '连接中…';
  try {
    await window.__TAURI__.core.invoke('select_service', { serverUrl: input.value.trim() });
  } catch (reason) {
    error.textContent = String(reason);
    button.disabled = false;
    input.disabled = false;
    button.textContent = '连接';
    input.focus();
  }
});
