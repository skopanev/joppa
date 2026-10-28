// The scope checkboxes of the board search and the rows of a scoped search. With All versions
// or All workspaces on, the board shows the search results themselves: a row links to its
// object, at its revision for a past revision, and in its workspace for another workspace.
import {E} from 'board/dom.js';
import {state} from 'board/state.js';
export function searchScope(controls,changed){
  const box=(label,name)=>{const wrap=E('label',undefined,'filter-check');const input=E('input');input.type='checkbox';input.name=name;input.onchange=changed;wrap.append(input,' '+label);controls.append(wrap);return input;};
  const versions=box('All versions','all_versions'),workspaces=box('All workspaces','all_workspaces');
  return ()=>({all_versions:versions.checked,all_workspaces:workspaces.checked});
}
export function searchHref(item){
  const params=new URLSearchParams({workspace:item.workspace,object:item.kind+':'+item.id});
  if(item.historical&&item.revision)params.set('revision',String(item.revision));
  return '/?'+params;
}
export function searchRows(items){
  return items.map(item=>{
    const row=E('a',undefined,'search-result');row.href=searchHref(item);row.dataset.address=item.kind+':'+item.id;row.dataset.workspace=item.workspace;
    const meta=[item.kind];if(item.workspace!==state.ws)meta.push(workspaceTitle(item.workspace));if(item.historical)meta.push('revision '+item.revision);if(item.archived)meta.push('archived');
    row.append(E('span',item.title,'search-title'),E('span',meta.join(' · '),'search-meta hint'));
    return row;
  });
}
// The workspace drop-down holds the title of each workspace that the reader can open.
function workspaceTitle(id){return [...document.querySelectorAll('#workspaces option')].find(o=>o.value===id)?.textContent||id;}
