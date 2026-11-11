// Board addresses of objects and the links to them.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {objectTitlePreview} from 'board/object-types.js';
export function objectURL(address,revision){
  const url=new URL(location.href);url.search='';url.hash='';
  url.searchParams.set('workspace',state.ws);
  if(address)url.searchParams.set('object',address);
  if(revision)url.searchParams.set('revision',revision);
  return url.href;
}
// The link that opens Flow view of the relation `key` with the node at `address` picked.
export function flowNodeURL(key,address){
  const url=new URL(location.href);url.hash='';
  url.search=new URLSearchParams({workspace:state.ws,view:'flow',relation:key,node:address}).toString();
  return url.href;
}
export function objectLink(title,address,revision){
  const link=E('a',objectTitlePreview(title,address.split(':')[0]),'object-link');link.setAttribute('aria-label',title);link.href=objectURL(address,revision);link.dataset.linkAddress=address;return link;
}
