# Herdr integration ownership

Desktop Pet's Herdr adapter, plugin manifest/actions, client-attach host patch and OMP outcome extension belong to the separate [Herdr integration repository](https://github.com/hanbong5938/herdr-desktop-pet), not this core checkout.

Use its [current host build, link, attach and OMP instructions](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md). Install or build the [independent native app](../../readme.md#installation-and-optional-integrations) first; link the integration checkout, never this core checkout. A host version floor alone does not establish `client.attached` support.

For immutable legacy instructions, read the [v0.3.6 guide at its original tag](https://github.com/hanbong5938/herdr-desktop-pet/blob/v0.3.6/integrations/herdr/README.md). Legacy installation commands are not neutral installation commands. Follow the [separate A → B migration](../../docs/migrations/desktop-pet.md) without moving or merging existing profiles.
