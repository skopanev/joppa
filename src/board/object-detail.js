// The body of an object: fields, links, dependencies, comments and files.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {FILE_ICON,svgIcon} from 'board/icons.js';
import {button} from 'board/feedback.js';
import {enableBranchMotion} from 'board/motion.js';
import {assessmentHint} from 'board/status.js';
import {personLabel,when,fileSize} from 'board/format.js';
import {inputQueue} from 'board/object-types.js';
import {objectLink} from 'board/links.js';
import {markdownBlock} from 'board/markdown.js';
import {criterionTestsSection} from 'board/criterion-tests.js';
import {workspaceLinksSection} from 'board/workspace-links.js';
import {incomingGroups} from 'board/link-groups.js';
export function fillCommonObject(body,detail){
  body.dataset.objectContextAddress=detail.address;
  const r=detail.document,d=r.data;
  const historical=detail.historical??r.revision!==detail.current_revision;
  const heading=body.parentElement?.querySelector(':scope > summary > .node-title,.history-dialog-header h2');
  const full=heading?E('div',undefined,'object-field object-full-title'):null;
  if(full){full.hidden=heading.dataset.truncated!=='true'&&heading.scrollHeight<=heading.clientHeight+1;full.append(E('small','Full title','type-label'),E('p',d.title,'node-description'));}
  if(historical)body.append(E('p','Saved document revision. Current status, comments, files and incoming links are not shown.','hint'));
  if(r.present===false)body.append(E('p','Historical object. This object is absent from the current tree.','dependency-review'));
  const secondary=E('div',undefined,'object-secondary');
  // The properties come first in the body, right under the row and above the children.
  // A view without the row Properties button, such as the revision dialog, always shows them.
  const fields=E('div',undefined,'object-details'),fieldsToggle=body.parentElement?.querySelector(':scope > summary .fields-button');
  fields.hidden=!!fieldsToggle&&fieldsToggle.getAttribute('aria-expanded')!=='true';body.prepend(fields);
  const meta=E('div',undefined,'object-meta');
  meta.append(E('span','Revision '+r.revision),E('span','Owner: '+personLabel(d.owner||'Unassigned')));fields.append(meta);
  if(full)fields.append(full);
  const verification=detail.process?.verification&&({pass:'Passed',fail:'Failed',unverified:'Unverified'}[detail.process.verification]||detail.process.verification);
  if(verification)fields.append(E('p','Verification: '+verification,'node-meta'));
  const status=typeof detail.process?.status==='string'?detail.process.status:detail.signal?.label;
  if(detail.process?.status&&!inputQueue(detail.kind)&&status!==verification&&status!=='Checks '+verification?.toLowerCase())fields.append(E('p','Status: '+status,'node-meta'));
  const assessment=detail.process?.readiness_blocker||detail.process?.assessment;
  if(!historical&&assessment)fields.append(E('p',assessmentHint(assessment).trim(),'node-meta'));
  if(detail.review_required)body.append(E('p','A dependency changed. Review the linked revision.','dependency-review'));
  const remote=historical?null:workspaceLinksSection(detail);
  if(remote)body.append(remote);
  const contentKey=['body','description','contract'].find(key=>typeof d[key]==='string'&&d[key].trim());
  if(contentKey)body.append(markdownBlock(d[contentKey],'object-content node-description'));
  for(const [key,value] of Object.entries(d)){
    if(['title','owner',contentKey].includes(key)||value===null||value===''||(Array.isArray(value)&&value.length===0))continue;
    const field=E('div',undefined,'object-field');
    const refRule=state.data.objectTypes?.[detail.kind]?.references?.[key];
    field.append(E('small',refRule?refRule.title:key.replaceAll('_',' '),'type-label'));
    if(refRule){
      const vals=Array.isArray(value)?value:[value];
      const refs=E('div',undefined,'object-links reference-links');
      for(const v of vals){
        const targetAddress=v.includes(':')?v:refRule.target+':'+v;
        const title=detail.references?.find(l=>l.target===targetAddress)?.title||detail.links?.find(l=>l.link.target===targetAddress)?.title||v;
        refs.append(objectLink(String(title),targetAddress));
      }
      field.append(refs);
    }else{
      field.append(E('p',typeof value==='string'?value:JSON.stringify(value),'node-description'));
    }
    fields.append(field);
  }
  // Blocking dependencies are shown read-only on the board.
  const deps=!historical&&r.present!==false&&body.parentElement?.classList.contains('common-object')?dependencyList(detail):null;
  if(deps)secondary.append(deps);
  if(!historical&&state.data.objectTypes?.[detail.kind]?.role==='ac')secondary.append(criterionTestsSection(detail.address));
  const links=new Map();
  for(const {link,title,current_revision,present} of detail.links||[]){const key=link.target+'@'+(link.revision||'');const label='Links to · '+link.relation+(link.revision?' · revision '+link.revision:'')+(link.revision&&current_revision!==link.revision&&detail.review_required===true?' · changed':'')+(present===false?' · historical object':'')+(historical?' · current title':'');const entry=links.get(key)||{address:link.target,title:title||link.target,revision:link.revision,labels:[]};entry.labels.push(label);links.set(key,entry);}
  if(links.size){
    const group=objectSection('Links '+links.size,'object-relations');
    const rows=E('div',undefined,'object-links');
    for(const entry of links.values()){const row=objectLink(entry.title,entry.address,entry.revision);row.append(E('small',[...new Set(entry.labels)].join(' · '),'object-link-meta'));rows.append(row);}
    group.append(rows);secondary.append(group);
  }
  if(!historical)secondary.append(...incomingGroups(detail,objectSection));
  const annotations=historical?{}:detail.annotations||{};
  if(annotations.description){const context=E('div',undefined,'object-field');context.append(E('small','Context','type-label'),E('p',annotations.description,'node-description'));fields.append(context);}
  const comments=Object.values(annotations.comments||{}).filter(comment=>!comment.withdrawn);
  if(comments.length){
    const group=objectSection(comments.length+' '+(comments.length===1?'comment':'comments'),'object-comments');
    for(const comment of comments){const row=E('article',undefined,'note');row.append(E('p',personLabel(comment.actor)+' · '+when(comment.at),'note-meta'),markdownBlock(comment.text,'note-text'));group.append(row);}
    secondary.append(group);
  }
  const files=historical?[]:Array.isArray(detail.files)?detail.files.map(file=>[file.id,file]):Object.entries(annotations.attachments||{}).filter(([,file])=>!file.removed);if(files.length){const group=objectSection('Files '+files.length,'object-files');group.append(attachmentList(files));secondary.append(group);}
  body.append(secondary);
}
export function objectSection(title,cls){
  const group=E('details',undefined,'object-section '+cls);
  group.append(E('summary',title));enableBranchMotion(group);return group;
}
// Blocking dependencies of a node, shown read-only on the board.
function dependencyList(detail){
  const blocking=(detail.links||[]).filter(({link})=>link.relation==='depends_on'&&link.blocking);
  if(!blocking.length)return null;
  const box=E('div',undefined,'object-dependencies');
  for(const {link,title} of blocking){
    const name=title||link.target,row=E('p',undefined,'dependency-row');
    row.append(E('span','Depends on','hint'),objectLink(name,link.target));box.append(row);
  }
  return box;
}
// Кнопка с цифрой. Цифра — это и есть ответ на вопрос «тут что-то обсуждали?»,
// ради которого иначе пришлось бы открывать каждый объект по очереди.
function attachmentList(attachments){
  const list=E('ul',undefined,'files-list');
  for(const [,a] of attachments){
    const li=E('li'),link=E('a',undefined,'file-link');
    link.href='/api/workspaces/'+encodeURIComponent(state.ws)+'/blobs/'+a.hash;
    link.target='_blank';link.rel='noopener';
    const icon=E('span',undefined,'file-icon');icon.append(svgIcon(FILE_ICON,22));
    const info=E('span',undefined,'file-info');
    info.append(E('span',a.name,'file-name'),E('span',[fileSize(a.size),personLabel(a.actor),when(a.at)].filter(Boolean).join(' · '),'file-meta'));
    link.append(icon,info,svgIcon('M7 17 17 7 M7 7h10v10',16));
    li.append(link);list.append(li);
  }
  return list;
}
