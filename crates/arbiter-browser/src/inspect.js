(e) => {
  if (!e) throw Error('No matching element');
  const clean = (v, n) => String(v || '').replace(/\s+/g, ' ').trim().slice(0, n);
  const selector = (el) => {
    if (el.id) return '#' + CSS.escape(el.id);
    const parts = [];
    for (let p = el; p && p.nodeType === 1 && parts.length < 6; p = p.parentElement) {
      if (p.id) { parts.unshift('#' + CSS.escape(p.id)); break; }
      const siblings = p.parentElement ? [...p.parentElement.children].filter(s => s.tagName === p.tagName) : [];
      parts.unshift(p.tagName.toLowerCase() + (siblings.length > 1 ? ':nth-of-type(' + (siblings.indexOf(p) + 1) + ')' : ''));
    }
    return parts.join(' > ');
  };
  let component = '', source = '';
  // Development metadata only. Never traverse props or serialize framework objects.
  for (let p = e, depth = 0; p && depth < 8; p = p.parentElement, depth++) {
    const key = Object.keys(p).find(k => k.startsWith('__reactFiber$'));
    for (let f = key && p[key], i = 0; f && i < 12; f = f.return, i++) {
      component ||= clean(f.type?.displayName || f.type?.name, 60);
      if (f._debugSource) source ||= clean(f._debugSource.fileName, 140) + ':' + (f._debugSource.lineNumber || 1);
    }
    const vue = p.__vueParentComponent;
    component ||= clean(vue?.type?.name || vue?.type?.__name, 60);
    source ||= clean(vue?.type?.__file, 160);
    const loc = p.__svelte_meta?.loc;
    if (loc) source ||= clean(loc.file, 140) + ':' + ((loc.line || 0) + 1);
    source ||= clean(p.getAttribute('data-source'), 160);
  }
  const r = e.getBoundingClientRect(), s = getComputedStyle(e), sel = selector(e);
  const styles = Object.fromEntries(['display','position','font-size','font-weight','color','background-color','padding','margin','border-radius','gap'].map(k => [k,clean(s.getPropertyValue(k),48)]));
  const box = {x:Math.round(r.x),y:Math.round(r.y),width:Math.round(r.width),height:Math.round(r.height)};
  const text = clean(e.innerText || e.getAttribute('aria-label') || '', 100);
  let descriptor = `Element ${clean(sel,220)}\n${component ? 'Component '+component+'\n' : ''}${source ? 'Source '+source+'\n' : ''}Text ${JSON.stringify(text)}\nBox ${box.x},${box.y} ${box.width}×${box.height}\n` + Object.entries(styles).map(([k,v]) => k+':'+v).join('; ');
  descriptor = descriptor.slice(0,1000);
  return {selector:sel,component:component || null,source:source || null,text,box,styles,descriptor};
}
