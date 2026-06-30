//! Build script for mabi-cli
//!
//! Displays the Mabinogion logo during compilation/installation.

fn main() {
    // Only show banner when building in release mode (typically during install)
    if std::env::var("PROFILE").unwrap_or_default() == "release" {
        println!(
            "cargo:warning=\x1b[38;5;208m
                             .:.
                           **=:=**.   *++**:
                          *=     :*+       *=
                          *:       -*=     +*
                          :.  .**    +=   :*:
                            .+*.        .*+
                           +*.        .*+
               -*+==+*:   +:   +*    +*.   *+    *****.
              +*.     +*.      .*.   *-     -*=      .*+
              *+       .*+     *+    +*       +*:     :*
               =   -*:   :*****:      -**+**    +*   .*+
                 -*+                                +*.
               :*+                                +*:
              +*.   ++    +****:      .+***+:   :*-   :
              *:     -*+       **    +*.    +*.       +*
              +*.      +*:     .*.   *.      .*+      *+
               .+****    +*    **    *+   :+   :*+--+*=
                             =*=        .*+
                           =*=        .*+.
                          *+   =+    **.  .:
                         +*     =*-       .*.
                         -*.      +*.     =*
                          -*++++.  .**=:=**
                                      .:.
\x1b[0m"
        );
        println!("cargo:warning=");
        println!("cargo:warning=  MABINOGION - Protocol Resilience Engine");
        println!("cargo:warning=");
    }
}
