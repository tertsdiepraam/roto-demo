use std::{
    path::{Path, PathBuf}, time::SystemTime
};

use bevy::{
    color::{Color, Mix, Srgba},
    ecs::resource::Resource,
    math::Vec3,
};
use rand::Rng;
use roto::{library, location, Item, Library, Registerable, Runtime, TypedFunc, Val, Type, Function};

use crate::{EMITTER, Particle};

type UpdateFn = fn(f32, Val<Particle>) -> Option<Val<Particle>>;
type AddFn = fn(f32);

#[derive(Resource)]
pub struct ScriptManager {
    pub runtime: Runtime,
    pub path: PathBuf,
    pub last_compile: SystemTime,
    pub script_not_found_logged: bool,
    pub update: Option<TypedFunc<(), UpdateFn>>,
    pub update_ms: f32,
    pub add: Option<TypedFunc<(), AddFn>>,
    pub add_ms: f32,
}

impl ScriptManager {
    pub fn new(path: &Path) -> Self {
        // A library collects all types, functions, etc. that are made available to the roto script
        let mut lib = Library::new();

        // We can add Rust types to the library.
        // The wrapper type `Val<T>` makes a registered type safe to pass to and from Roto. 
        // We can rename the added type, in this example from Val<Particle> to Particle.
        // Add a description for the docs that Roto can generate.
        // Lastly, a location for error reporting during item registration (usually just the location! macro)
        lib.add(
            Item::Type(
                Type::clone::<Val<Particle>>("Particle", "This is a Particle!", location!()).unwrap()
            )
        );

        fn emit_new_particles(particle: Val<Particle>) {
            EMITTER.lock().unwrap().push(particle.0);
        }

        // We can add functions too!
        // Just as before we can (re-)name the function, add a description and aid the error reporting,
        // Here we rename the function to just "emit", so Roto scripts can call `emit(Particle)`!
        lib.add(
            Item::Function(
                Function::new("emit", "Emit a new particle", vec!["particle"], emit_new_particles, location!()).unwrap()
            )
        );
    
        // As you can see in the two examples above, this can get very tedious.
        // Luckily there is the `library!` macro, that makes this process feel very natrual to Rust!
        let lib_via_macro = library! {
            #[copy] type Vec3 = Val<Vec3>;
            #[copy] type Color = Val<Color>;

            impl Val<Particle> {
                fn new(pos: Val<Vec3>, scale: f32, color: Val<Color>) -> Self {
                    Val(Particle { pos: pos.0, scale, color: color.0 })
                }

                fn pos(self) -> Val<Vec3> {
                    Val(self.pos)
                }

                fn scale(self) -> f32 {
                    self.scale
                }

                fn color(self) -> Val<Color> {
                    Val(self.color)
                }
            }

            impl Val<Vec3> {
                fn new(x: f32, y: f32, z: f32) -> Self {
                    Val(Vec3 { x, y, z })
                }

                fn add(self, other: Self) -> Self {
                    Val(self.0 + other.0)
                }

                fn x(self) -> f32 {
                    self.x
                }

                fn y(self) -> f32 {
                    self.y
                }

                fn z(self) -> f32 {
                    self.z
                }

                fn length(self) -> f32 {
                    self.length()
                }

                fn normalize(self) -> Self {
                    Val(self.normalize())
                }

                fn scale(self, r: f32) -> Self {
                    Val(self.0 * r)
                }
            }

            impl Val<Color> {
                fn red() -> Self {
                    Val(Color::from(Srgba::RED))
                }

                fn none() -> Self {
                    Val(Color::from(Srgba::NONE))
                }

                fn new(r: f32, g: f32, b: f32) -> Self {
                    Val(Color::from(Srgba::new(r, g, b, 1.0)))
                }

                fn mix(t: f32, x: Self, y: Self) -> Self {
                    Val(x.mix(&y, t))
                }
            }

            impl f32 {
                fn rand(low: f32, high: f32) -> f32 {
                    let mut rng = rand::rng();
                    rng.random_range(low..high)
                }

                fn sin(self) -> Self {
                    self.sin()
                }

                fn cos(self) -> Self {
                    self.cos()
                }

                fn pi() -> f32 {
                    std::f32::consts::PI
                }
            }
        };

        // Add the two libraries together
        lib_via_macro.add_to_lib(&mut lib);

        // Build the Roto runtime
        let mut runtime = Runtime::from_lib(lib).unwrap();
        runtime.add_io_functions();

        Self {
            runtime,
            path: path.to_path_buf(),
            last_compile: SystemTime::UNIX_EPOCH,
            script_not_found_logged: false,
            update: None,
            update_ms: 0.0,
            add: None,
            add_ms: 0.0,
        }
    }

    /// Check the modification time of the script and reload it if it is outdated.
    pub fn reload(&mut self) {
        let res = std::fs::metadata(&self.path);

        let modified = match res.and_then(|md| md.modified()) {
            Ok(modified) => modified,
            Err(e) => {
                self.last_compile = SystemTime::now();
                if !self.script_not_found_logged {
                    eprintln!("Script not found: {e}");
                    self.script_not_found_logged = true;
                }
                return;
            }
        };

        self.script_not_found_logged = false;

        if self.last_compile > modified {
            // We last checked this later than it was modified, just continue
            // with the current version
            return;
        }

        self.last_compile = SystemTime::now();

        let res = self.runtime.compile(&self.path);

        let mut pkg = match res {
            Ok(pkg) => pkg,
            Err(e) => {
                // Print any compilation errors
                println!("{e}");
                return;
            }
        };

        if let Ok(update) = pkg.get_function("update") {
            self.update = Some(update);
        }

        if let Ok(add) = pkg.get_function("add") {
            self.add = Some(add);
        }
    }
}
