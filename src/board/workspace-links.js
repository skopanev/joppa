// The parts of an object that point to other workspaces: a child workspace and remote links.
// The server reads both with the access of the viewer; the board only shows them.
import {E} from 'board/dom.js';

const STATUS={green:'all criteria green',red:'a criterion is red',grey:'not all criteria green'};
// A tone without a label still shows its name, never "undefined".
const statusLabel=status=>STATUS[status]||'status '+status;

function workspaceLink(id,address){
  const link=E('a',address?id+' / '+address:id);
  link.href='/?workspace='+encodeURIComponent(id)+(address?'&object='+encodeURIComponent(address):'');
  return link;
}

export function workspaceLinksSection(detail){
  const child=detail.child_workspace,remote=detail.remote_links||[];
  if(!child&&!remote.length)return null;
  const section=E('div',undefined,'object-field workspace-links');
  // A changed or archived target is shown, never ignored.
  if(detail.remote_review_required)section.append(E('p','A linked object in another workspace changed or was archived. Review the link.','dependency-review'));
  if(child){
    section.append(E('small','Child workspace','type-label'));
    const line=E('p',undefined,'node-description');
    line.append(workspaceLink(child.workspace),' · ');
    line.append(child.access?statusLabel(child.status)+' ('+child.criteria.green+' of '+child.criteria.total+')':'no access');
    section.append(line);
  }
  if(remote.length){
    section.append(E('small','Links to other workspaces','type-label'));
    for(const link of remote){
      const s=link.status||{},line=E('p',undefined,'node-description');
      line.append(link.relation+(link.blocking?' (blocking)':'')+': ',workspaceLink(link.workspace,link.target));
      if(!s.access)line.append(' · no access');
      else if(s.present===false)line.append(' · archived or absent');
      else{
        // A blocking link is ready when its target is green and its pin is current.
        if(link.blocking)line.append(s.satisfied?' · ready':s.review_required?' · not ready, pin stale':' · not ready');
        if(s.review_required)line.append(' · changed: revision '+link.revision+' → '+s.current_revision);
      }
      section.append(line);
    }
  }
  return section;
}
