(() => {
  const issues = [], seen = new Set();
  const add = (message) => { if (!seen.has(message) && issues.length < 30) { seen.add(message); issues.push(message.slice(0,240)); } };
  const visible = e => { const r=e.getBoundingClientRect(),s=getComputedStyle(e);return r.width>0 && r.height>0 && s.visibility!=='hidden' && !e.closest('[aria-hidden="true"],[inert]'); };
  const name = e => (e.getAttribute('aria-label') || (e.getAttribute('aria-labelledby') || '').split(/\s+/).map(id=>document.getElementById(id)?.textContent || '').join(' ') || [...(e.labels || [])].map(l=>l.textContent).join(' ') || e.getAttribute('title') || '').trim();
  const id = e => e.tagName.toLowerCase()+(e.id ? '#'+e.id : e.getAttribute('name') ? '[name="'+e.getAttribute('name')+'"]' : '');
  if (!document.documentElement.lang.trim()) add('html: missing document language');
  if (!document.title.trim()) add('document: missing page title');
  for (const e of document.querySelectorAll('img,button,input,textarea,select,a[href],[role="button"]')) {
    if (!visible(e)) continue;
    if (e.matches('img') && !e.hasAttribute('alt') && !name(e) && e.getAttribute('role')!=='presentation') add(id(e)+': missing alternative text');
    if (e.matches('button,a[href],[role="button"]') && !name(e) && !e.textContent.trim() && !e.querySelector('img[alt]:not([alt=""]),[aria-label]')) add(id(e)+': missing accessible name');
    if (e.matches('input:not([type="hidden"]):not([type="submit"]):not([type="reset"]):not([type="button"]),textarea,select') && !name(e)) add(id(e)+': missing field label');
  }
  return issues;
})()
