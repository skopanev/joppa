// Text formats for people, dates and file sizes.
import {state} from 'board/state.js';
export function displayPersonName(value){
  const name=String(value||'').trim();
  if(!name)return 'Not assigned';
  const local=name.includes('@')?name.split('@')[0].split('+')[0]:name;
  if(!/^[\p{L}]+(?:[._][\p{L}]+)+$/u.test(local))return local;
  return local.split(/[._]/).map(part=>part[0].toLocaleUpperCase()+part.slice(1)).join(' ');
}
export function personLabel(actor){
  return displayPersonName(state.data.member_labels?.[actor]||actor);
}
export function eventDate(value){
  const date=new Date(value.replace(' ','T').replace(/(\.\d{3})\d+/,'$1').replace(/([+-]\d{2})$/,'$1:00'));
  return Number.isNaN(date.getTime())?'Date unavailable':new Intl.DateTimeFormat('en-GB',{day:'numeric',month:'short',year:'numeric',hour:'2-digit',minute:'2-digit'}).format(date);
}
export function when(at){
  if(!at)return '';
  const d=new Date(at.replace(' ','T').replace(/\+00$/,'Z'));
  return isNaN(d)?at:d.toLocaleString(undefined,{dateStyle:'medium',timeStyle:'short'});
}
export function fileSize(bytes){
  if(bytes<1024)return bytes+' B';
  if(bytes<1024*1024)return Math.round(bytes/1024)+' KB';
  return (bytes/(1024*1024)).toFixed(1)+' MB';
}
