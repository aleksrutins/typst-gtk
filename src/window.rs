/* window.rs
 *
 * Copyright 2023 Aleks Rutins
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use gtk::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/com/rutins/Typst/window.ui")]
    pub struct TypstGtkWindow {
        // Template widgets
        #[template_child]
        pub header_bar: TemplateChild<gtk::HeaderBar>,
        #[template_child]
        pub label: TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TypstGtkWindow {
        const NAME: &'static str = "TypstGtkWindow";
        type Type = super::TypstGtkWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for TypstGtkWindow {}
    impl WidgetImpl for TypstGtkWindow {}
    impl WindowImpl for TypstGtkWindow {}
    impl ApplicationWindowImpl for TypstGtkWindow {}
    impl AdwApplicationWindowImpl for TypstGtkWindow {}
}

glib::wrapper! {
    pub struct TypstGtkWindow(ObjectSubclass<imp::TypstGtkWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,        @implements gio::ActionGroup, gio::ActionMap;
}

impl TypstGtkWindow {
    pub fn new<P: glib::IsA<gtk::Application>>(application: &P) -> Self {
        glib::Object::builder()
            .property("application", application)
            .build()
    }
}
