const form = document.getElementById('service-form');
const input = document.getElementById('service-url');
const error = document.getElementById('error');

form.addEventListener('submit', async (event) => {
  event.preventDefault();
  input.disabled = true;
  error.textContent = '';
  try {
    await window.__TAURI__.core.invoke('select_service', { serverUrl: input.value.trim() });
  } catch (reason) {
    error.textContent = String(reason);
    input.disabled = false;
    input.focus();
  }
});
