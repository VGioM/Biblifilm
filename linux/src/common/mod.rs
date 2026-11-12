//! Le socle commun aux trois apps Linux (Biblifilm, Biblijeu, Recomps) : le
//! même dossier `common/` vit dans chacune, une correction faite ici se reporte
//! dans les deux autres.

#![allow(dead_code)]

pub mod icons;
pub mod images;
pub mod selfupdate;
pub mod store;
pub mod sys;
pub mod testhook;
pub mod theme;
pub mod ui;
