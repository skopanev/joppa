// Safe Markdown display of object text.
import {E} from 'board/dom.js';
import {objectURL} from 'board/links.js';
// Object text is Markdown. Line breaks stay, active HTML is removed, and external links open safely.
// [Title](kind:id) is a link to an object of this workspace: the href becomes the same-origin object
// URL before DOMPurify checks the URI, so no other scheme is allowed.
const OBJECT_ADDRESS=/^[a-z][a-z0-9_-]*:[a-zA-Z0-9_.-]+$/;
const markdownSafe=(()=>{
  if(!window.DOMPurify||!window.marked)return null;
  DOMPurify.addHook('uponSanitizeAttribute',(node,data)=>{
    const address=data.attrValue.trim();
    if(node.tagName==='A'&&data.attrName==='href'&&OBJECT_ADDRESS.test(address)){data.attrValue=objectURL(address);node.setAttribute('data-link-address',address);}
  });
  DOMPurify.addHook('afterSanitizeAttributes',node=>{
    if(node.tagName==='A'){
      const href=node.getAttribute('href')||'';
      if(/^\s*(javascript|data|vbscript):/i.test(href))node.removeAttribute('href');
      else if(/^https?:/i.test(href)&&new URL(href,location.href).origin!==location.origin){node.setAttribute('target','_blank');node.setAttribute('rel','noopener noreferrer');}
    }
    if(node.tagName==='INPUT')node.setAttribute('disabled','');
  });
  return text=>DOMPurify.sanitize(marked.parse(text,{gfm:true,breaks:true}),
    {FORBID_TAGS:['style','iframe','svg','math','form','object','embed'],FORBID_ATTR:['style']});
})();
export function markdownBlock(text,cls){
  const box=E('div',undefined,'markdown '+cls);
  if(markdownSafe)box.innerHTML=markdownSafe(text);else{box.textContent=text;box.classList.add('markdown-raw');}
  return box;
}
